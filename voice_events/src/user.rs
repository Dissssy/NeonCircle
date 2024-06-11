use crate::{commands::WithFeedback, transcribe};

use super::{
    human_readable_bytes, InnerThreadCommand, PacketData, PacketDuration, ThreadResponse,
    ThreadResponseAction, MIN_SAMPLES_FOR_TRANSCRIPTION,
};
use common::{
    anyhow::{anyhow, Result},
    log,
    serenity::{
        all::{Http, UserId},
        futures::{stream::FuturesUnordered, StreamExt as _},
    },
    tokio::{self, sync::mpsc, task::JoinHandle, time::Instant},
    utils::OptionalTimeout,
};
use std::{sync::Arc, time::Duration};
pub struct TranscriptionThread {
    handle: tokio::task::JoinHandle<()>,
    pub command: mpsc::UnboundedSender<InnerThreadCommand>,
    pub user_id: UserId,
}
impl TranscriptionThread {
    pub fn new(
        user_id: UserId,
        responses: mpsc::UnboundedSender<ThreadResponse>,
        http: Arc<Http>,
    ) -> Self {
        let (command, rx) = mpsc::unbounded_channel();
        let handle = tokio::task::spawn(Self::user_thread(user_id, rx, responses, http));
        Self {
            handle,
            command,
            user_id,
        }
    }
    pub fn send(&self, packet: PacketData) {
        if let Err(e) = self.command.send(InnerThreadCommand::Process(packet)) {
            log::error!("Failed to send packet to thread: {:?}", e);
        }
    }
    pub async fn stop(self) {
        if let Err(e) = self.command.send(InnerThreadCommand::Stop) {
            log::error!("Failed to send stop command to thread: {:?}", e);
        }
        if let Err(e) = self.handle.await {
            log::error!("Failed to join thread: {:?}", e);
        }
    }
    async fn user_thread(
        user_id: UserId,
        mut rx: mpsc::UnboundedReceiver<InnerThreadCommand>,
        responses: mpsc::UnboundedSender<ThreadResponse>,
        http: Arc<Http>,
    ) {
        let mut buffer = Vec::new();
        let mut last_received: Option<Instant> = None;
        let mut timeout = OptionalTimeout::new(Duration::from_millis(750));
        let mut pending: FuturesUnordered<JoinHandle<Result<ThreadResponse>>> =
            FuturesUnordered::new();
        loop {
            tokio::select! {
                _ = &mut timeout => {
                    log::trace!("{} is done talking with {} of data", user_id, human_readable_bytes(buffer.len() * std::mem::size_of::<u8>()));
                    let mut buf = Vec::new();
                    std::mem::swap(&mut buffer, &mut buf);
                    if buf.len() > MIN_SAMPLES_FOR_TRANSCRIPTION {
                        let http = Arc::clone(&http);
                        pending.push(tokio::spawn(async move {
                            Self::transcribe(&buf, user_id, http).await
                        }));
                    } else {
                        log::trace!("{} did not talk long enough", user_id);
                    }
                    last_received = None;
                }
                Some(command) = rx.recv() => {
                    match command {
                        InnerThreadCommand::Stop => {
                            log::trace!("Stopping thread for {}", user_id);
                            break;
                        }
                        InnerThreadCommand::Process(packet) => {
                            if packet.user_id != user_id {
                                log::error!("Received packet for wrong user");
                                continue;
                            }
                            if let Some(last_received) = last_received {
                                // we're going to handle detecting timeouts seperately, but here i just want to pad out the buffer with silence to fill voids larger than 20ms
                                let duration = packet.received.duration_since(last_received);
                                if duration.as_millis() > 30 {
                                    let duration = PacketDuration::from_dur(duration - Duration::from_millis(20));
                                    let count = duration.to_packet_count();
                                    log::trace!("{} has a gap of {}ms", user_id, duration.as_millis());
                                    buffer.extend(std::iter::repeat(0).take(count));
                                }
                            } else {
                                log::trace!("{} began speaking", user_id);
                                buffer.clear();
                            }
                            last_received = Some(packet.received);
                            timeout.begin_now();
                            buffer.extend(packet.audio);
                        }
                    }
                }
                Some(audio) = pending.next() => {
                    match audio {
                        Ok(Ok(resp)) => {
                            if let Err(e) = responses.send(resp) {
                                log::error!("Failed to send response to main thread: {}", e);
                            }
                        }
                        Ok(Err(e)) => {
                            log::error!("Failed to convert audio to mp3: {}", e);
                        }
                        Err(e) => {
                            log::error!("Failed to get audio future: {}", e);
                        }
                    }
                }
            }
        }
    }

    async fn transcribe(audio: &[i16], user_id: UserId, http: Arc<Http>) -> Result<ThreadResponse> {
        // pcm_s16le_to_mp3(&buf).await
        let resp = transcribe(audio).await?;
        let content = resp.to_string();

        let response = super::commands::parse_commands(&content, user_id, http).await;

        Ok(ThreadResponse { response, user_id })
    }
}
