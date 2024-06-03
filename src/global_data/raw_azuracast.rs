use crate::radio::{AzuraCastThread, OriginalOrCustom};
use std::{mem::MaybeUninit, sync::Arc};
use tokio::sync::broadcast;
static mut AZURACAST: MaybeUninit<AzuraCastThread> = MaybeUninit::uninit();
static mut INITIALIZED: bool = false;
pub async fn init() {
    unsafe {
        if INITIALIZED {
            return;
        }
        match AzuraCastThread::new().await {
            Ok(a) => AZURACAST.write(a),
            Err(e) => {
                log::error!("Error initializing AzuraCast: {}", e);
                return;
            }
        };
        INITIALIZED = true;
    }
}
pub async fn resubscribe() -> broadcast::Receiver<Arc<OriginalOrCustom>> {
    while unsafe { !INITIALIZED } {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    unsafe { AZURACAST.assume_init_mut() }.resubscribe().await
}
// pub async fn signal_resend() {
//     if unsafe { !INITIALIZED } {
//         return;
//     }
//     unsafe { AZURACAST.assume_init_mut() }.signal_resend().await
// }
pub async fn save() {
    if unsafe { !INITIALIZED } {
        return;
    }
    unsafe { INITIALIZED = false };
    unsafe { AZURACAST.assume_init_read() }.kill().await;
}
