use crate::{commands::music::VideoType, video::Video};
use common::log;
use common::serenity::{async_trait, FutureExt as _};
use songbird::{Event, EventContext};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::{
    sync::RwLock,
    time::{Instant, Sleep},
};
// this struct will implement the Future trait, and only ever return if there is an active timeout and it has expired, turning the timeout into a None
pub struct OptionalTimeout {
    // from: Option<Instant>,
    opt: Option<(Pin<Box<Sleep>>, Instant)>,
    duration: Duration,
}
impl OptionalTimeout {
    pub fn new(duration: Duration) -> Self {
        // Self {
        //     from: None,
        //     duration,
        // }
        Self {
            opt: None,
            duration,
        }
    }
    pub fn set_duration(&mut self, duration: Duration) {
        if let Some((ref mut fut, began)) = self.opt {
            fut.as_mut().reset(began + duration);
        }
        self.duration = duration;
    }
    pub fn begin_now(&mut self) {
        let duration = self.duration;
        self.opt = Some((Box::pin(tokio::time::sleep(duration)), Instant::now()));
    }
    pub fn end_now(&mut self) {
        self.opt = None;
    }
}
impl Future for OptionalTimeout {
    type Output = ();
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        let poll = match self.as_mut().opt {
            None => std::task::Poll::Pending,
            Some((ref mut fut, _)) => fut.poll_unpin(cx),
        };
        if poll.is_ready() {
            self.as_mut().end_now();
        }
        poll
    }
}
pub struct DeleteAfterFinish {
    audio: Arc<RwLock<Option<VideoType>>>,
}
impl DeleteAfterFinish {
    pub fn new(audio: VideoType) -> Self {
        Self {
            audio: Arc::new(RwLock::new(Some(audio))),
        }
    }
    pub fn new_disk(audio: Video) -> Self {
        Self::new(VideoType::Disk(audio))
    }
    // pub fn new_url(audio: VideoInfo) -> Self {
    //     Self::new(VideoType::Url(audio))
    // }
}
#[async_trait]
impl songbird::EventHandler for DeleteAfterFinish {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(t) = ctx {
            if t.iter().any(|(state, _)| state.playing.is_done()) {
                log::trace!("Track finished, deleting audio");
                if let Some(audio) = self.audio.write().await.take() {
                    drop(audio);
                }
            } else {
                log::trace!("Track not finished");
            }
        }
        None
    }
}
