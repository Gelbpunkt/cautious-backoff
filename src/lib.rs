#![deny(clippy::pedantic, clippy::nursery)]

use std::time::Duration;

use tokio::{
    sync::{mpsc, oneshot},
    time::sleep,
};

#[derive(Clone)]
pub struct CautiousBackoff(mpsc::UnboundedSender<oneshot::Sender<Permit>>);

impl CautiousBackoff {
    #[must_use]
    pub fn new(initial_wait: Duration, max_wait: Duration, time_between_permits: Duration) -> Self {
        let (sender, recv) = mpsc::unbounded_channel();
        tokio::spawn(backoff(recv, initial_wait, max_wait, time_between_permits));

        Self(sender)
    }

    #[must_use = "permits should be used to signal outcome of task"]
    pub async fn wait(&self) -> Permit {
        let (tx, rx) = oneshot::channel();

        unsafe {
            // SAFETY: This will only fail if backoff task terminates, which
            // happens only when recv fails, aka all senders are dropped.
            // Impossible since we have one!
            self.0.send(tx).unwrap_unchecked();

            // SAFETY: We never drop the sender
            rx.await.unwrap_unchecked()
        }
    }
}

async fn backoff(
    mut recv: mpsc::UnboundedReceiver<oneshot::Sender<Permit>>,
    initial: Duration,
    cap: Duration,
    time_between_permits: Duration,
) {
    let mut current_retry_interval = initial;

    loop {
        let Some(tx) = recv.recv().await else {
            return;
        };

        let (outcome_tx, outcome_rx) = oneshot::channel();

        // SAFETY: We never drop the receiver
        unsafe { tx.send(Permit(outcome_tx)).unwrap_unchecked() };

        if let Ok(outcome) = outcome_rx.await {
            match outcome {
                Outcome::Fail => {
                    current_retry_interval *= 2;

                    if current_retry_interval > cap {
                        current_retry_interval = cap;
                    }

                    sleep(current_retry_interval).await;
                }
                Outcome::Success => {
                    current_retry_interval = initial;

                    sleep(time_between_permits).await;
                }
            }
        }
    }
}

pub enum Outcome {
    Success,
    Fail,
}

pub struct Permit(oneshot::Sender<Outcome>);

impl Permit {
    pub fn success(self) {
        let _ = self.0.send(Outcome::Success);
    }

    pub fn fail(self) {
        let _ = self.0.send(Outcome::Fail);
    }
}
