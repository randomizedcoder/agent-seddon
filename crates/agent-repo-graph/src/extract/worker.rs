use super::{deadline, extract_with_cancel};
use crate::{ExtractBudget, ExtractReport, RepoGraph, RepoGraphError, RepoGraphResult};
use std::{
    future::Future,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::Semaphore;

/// One blocking parser admission slot, held by the parser even after its caller times out
/// or is canceled. Busy requests fail immediately instead of accumulating blocking jobs.
#[derive(Clone)]
pub struct ExtractionWorker {
    slot: Arc<Semaphore>,
}

struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl Default for ExtractionWorker {
    fn default() -> Self {
        Self {
            slot: Arc::new(Semaphore::new(1)),
        }
    }
}

impl ExtractionWorker {
    /// Extract on a blocking worker. Late results are discarded and cannot be published.
    pub async fn extract(
        &self,
        root: PathBuf,
        budget: ExtractBudget,
    ) -> RepoGraphResult<(RepoGraph, ExtractReport)> {
        self.run(budget.deadline, move |cancel| {
            extract_with_cancel(&root, &budget, Some(cancel))
        })
        .await
    }

    /// Extract a normalized subset using the same bounded, cancelable worker.
    pub async fn extract_selected(
        &self,
        root: PathBuf,
        budget: ExtractBudget,
        modes: [bool; 3],
    ) -> RepoGraphResult<(RepoGraph, ExtractReport)> {
        self.run(budget.deadline, move |cancel| {
            super::extract_selected(&root, &budget, modes, Some(cancel))
        })
        .await
    }

    async fn run<T: Send + 'static>(
        &self,
        until: std::time::Instant,
        parse: impl FnOnce(Arc<AtomicBool>) -> RepoGraphResult<T> + Send + 'static,
    ) -> RepoGraphResult<T> {
        let budget = ExtractBudget {
            deadline: until,
            ..Default::default()
        };
        deadline(&budget)?;
        let permit = self
            .slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| RepoGraphError::Conflict("extraction worker busy".into()))?;
        let cancel = Arc::new(AtomicBool::new(false));
        let _cancel_on_drop = CancelOnDrop(cancel.clone());
        let job = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            parse(cancel)
        });
        let result = timeout(until, job).await??;
        deadline(&budget)?;
        result
    }
}

async fn timeout<T>(
    until: std::time::Instant,
    job: impl Future<Output = Result<T, tokio::task::JoinError>>,
) -> RepoGraphResult<RepoGraphResult<T>> {
    tokio::time::timeout_at(until.into(), job)
        .await
        .map_err(|_| RepoGraphError::Backend("extraction deadline".into()))
        .map(|r| r.map_err(|_| RepoGraphError::Backend("extraction worker failed".into())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    #[tokio::test]
    async fn boundary_deadline_late_worker() {
        let worker = ExtractionWorker::default();
        let (release, blocked) = mpsc::channel();
        let (entered, started) = tokio::sync::oneshot::channel();
        let w = worker.clone();
        let task = tokio::spawn(async move {
            w.run(Instant::now() + Duration::from_secs(1), move |_| {
                entered.send(()).unwrap();
                blocked.recv().unwrap();
                Ok(42)
            })
            .await
        });
        started.await.unwrap();
        assert!(matches!(
            task.await.unwrap(),
            Err(RepoGraphError::Backend(_))
        ));
        assert!(matches!(
            worker
                .run(Instant::now() + Duration::from_secs(10), |_| Ok(0))
                .await,
            Err(RepoGraphError::Conflict(_))
        ));
        release.send(()).unwrap();
        let permit =
            tokio::time::timeout(Duration::from_secs(10), worker.slot.clone().acquire_owned())
                .await
                .unwrap()
                .unwrap();
        drop(permit);
        assert_eq!(
            worker
                .run(Instant::now() + Duration::from_secs(10), |_| Ok(7))
                .await
                .unwrap(),
            7
        );
    }

    #[tokio::test]
    async fn corner_canceled_worker_holds_admission() {
        let worker = ExtractionWorker::default();
        let (release, blocked) = mpsc::channel();
        let (entered, started) = tokio::sync::oneshot::channel();
        let (finished, did_finish) = tokio::sync::oneshot::channel();
        let w = worker.clone();
        let task = tokio::spawn(async move {
            w.run(Instant::now() + Duration::from_secs(30), move |cancel| {
                entered.send(()).unwrap();
                blocked.recv().unwrap();
                finished.send(cancel.load(Ordering::Relaxed)).unwrap();
                Ok(42)
            })
            .await
        });
        started.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(matches!(
            worker
                .run(Instant::now() + Duration::from_secs(10), |_| Ok(0))
                .await,
            Err(RepoGraphError::Conflict(_))
        ));
        release.send(()).unwrap();
        assert!(did_finish.await.unwrap());
    }
}
