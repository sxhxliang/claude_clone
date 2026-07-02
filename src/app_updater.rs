use gpui::{Context, Task};
use gpui_updater::{EngineConfig, Release, UpdateEngine, UpdateSource, Version};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

type BoxedEngine = UpdateEngine<Box<dyn UpdateSource>>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Version),
    Downloading {
        downloaded: u64,
        total: Option<u64>,
    },
    Installing,
    Staged(Version),
    Errored(String),
}

impl UpdateStatus {
    fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading { .. } | Self::Installing
        )
    }
}

pub(crate) struct Updater {
    status: UpdateStatus,
    available: Option<Release>,
    engine: Arc<BoxedEngine>,
    task: Option<Task<()>>,
}

impl Updater {
    pub(crate) fn new<S: UpdateSource>(
        source: S,
        config: EngineConfig,
        _cx: &mut Context<Self>,
    ) -> Self {
        let engine = UpdateEngine::new(Box::new(source) as Box<dyn UpdateSource>, config);
        Self {
            status: UpdateStatus::Idle,
            available: None,
            engine: Arc::new(engine),
            task: None,
        }
    }

    pub(crate) fn status(&self) -> &UpdateStatus {
        &self.status
    }

    fn set_status(&mut self, status: UpdateStatus, cx: &mut Context<Self>) {
        self.status = status;
        cx.notify();
    }

    pub(crate) fn check(&mut self, cx: &mut Context<Self>) {
        if self.status.is_busy() {
            return;
        }

        self.set_status(UpdateStatus::Checking, cx);
        let engine = self.engine.clone();
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { engine.check() })
                .await;

            this.update(cx, |this, cx| {
                this.task = None;
                match result {
                    Ok(Some(release)) => {
                        let version = release.version.clone();
                        this.available = Some(release);
                        this.set_status(UpdateStatus::Available(version), cx);
                    }
                    Ok(None) => this.set_status(UpdateStatus::UpToDate, cx),
                    Err(error) => this.set_status(UpdateStatus::Errored(error.to_string()), cx),
                }
            })
            .ok();
        }));
    }

    pub(crate) fn download_and_install(&mut self, cx: &mut Context<Self>) {
        if self.status.is_busy() {
            return;
        }

        let Some(release) = self.available.clone() else {
            return;
        };

        let engine = self.engine.clone();
        self.set_status(
            UpdateStatus::Downloading {
                downloaded: 0,
                total: None,
            },
            cx,
        );

        self.task = Some(cx.spawn(async move |this, cx| {
            let downloaded = Arc::new(AtomicU64::new(0));
            let total = Arc::new(AtomicU64::new(0));
            let done = Arc::new(AtomicBool::new(false));

            let download_task = {
                let engine = engine.clone();
                let release = release.clone();
                let downloaded = downloaded.clone();
                let total = total.clone();
                let done = done.clone();
                cx.background_executor().spawn(async move {
                    let result = engine.download(&release, |got, maybe_total| {
                        downloaded.store(got, Ordering::Relaxed);
                        total.store(maybe_total.unwrap_or(0), Ordering::Relaxed);
                    });
                    done.store(true, Ordering::Relaxed);
                    result
                })
            };

            loop {
                let got = downloaded.load(Ordering::Relaxed);
                let maybe_total = total.load(Ordering::Relaxed);
                this.update(cx, |this, cx| {
                    this.set_status(
                        UpdateStatus::Downloading {
                            downloaded: got,
                            total: (maybe_total != 0).then_some(maybe_total),
                        },
                        cx,
                    );
                })
                .ok();

                if done.load(Ordering::Relaxed) {
                    break;
                }

                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
            }

            let artifact = match download_task.await {
                Ok(path) => path,
                Err(error) => {
                    this.update(cx, |this, cx| {
                        this.task = None;
                        this.set_status(UpdateStatus::Errored(error.to_string()), cx);
                    })
                    .ok();
                    return;
                }
            };

            this.update(cx, |this, cx| this.set_status(UpdateStatus::Installing, cx))
                .ok();

            let installed = {
                let engine = engine.clone();
                cx.background_executor()
                    .spawn(async move { engine.install(&artifact) })
                    .await
            };

            this.update(cx, |this, cx| {
                this.task = None;
                match installed {
                    Ok(installed) => {
                        if let Some(path) = &installed.restart_path {
                            cx.set_restart_path(path.clone());
                        }
                        this.set_status(UpdateStatus::Staged(release.version.clone()), cx);
                    }
                    Err(error) => this.set_status(UpdateStatus::Errored(error.to_string()), cx),
                }
            })
            .ok();
        }));
    }

    pub(crate) fn restart(&self, cx: &mut Context<Self>) {
        cx.restart();
    }
}
