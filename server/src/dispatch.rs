//! Send queue: requests enqueue a job and return; background tasks deliver it to every target
//! channel. A failing channel is logged and counted, and never stops the queue.

use crate::{
    channels::{Kind, Message, SendCtx},
    db::Channel,
    error::AppError,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::{
    sync::{mpsc, Semaphore},
    task::JoinSet,
};

const QUEUE_CAPACITY: usize = 10_000;
/// Jobs one user may have waiting or in delivery, so a single token cannot fill the queue.
const MAX_PENDING_PER_USER: usize = 500;
/// Message size limits, in bytes (extras as JSON).
const MAX_TITLE: usize = 2 * 1024;
const MAX_BODY: usize = 64 * 1024;
const MAX_EXTRAS: usize = 16 * 1024;

#[derive(Debug)]
pub struct Job {
    pub user_id: i64,
    /// Only this channel; all of the user's active channels when `None`.
    pub channel_key: Option<String>,
    pub msg: Message,
}

type PendingCounts = Arc<Mutex<HashMap<i64, usize>>>;

/// Counts a job against its user until dropped (delivered, or refused by the queue).
struct Pending {
    counts: PendingCounts,
    user_id: i64,
}

impl Drop for Pending {
    fn drop(&mut self) {
        let mut counts = self.counts.lock().unwrap();
        if let Some(n) = counts.get_mut(&self.user_id) {
            *n -= 1;
            if *n == 0 {
                counts.remove(&self.user_id);
            }
        }
    }
}

#[derive(Clone)]
pub struct Dispatcher {
    tx: mpsc::Sender<(Job, Pending)>,
    pending: PendingCounts,
}

impl Dispatcher {
    pub fn start(ctx: Arc<SendCtx>, concurrency: usize) -> Self {
        let (tx, mut rx) = mpsc::channel::<(Job, Pending)>(QUEUE_CAPACITY);
        let limit = Arc::new(Semaphore::new(concurrency));
        tokio::spawn(async move {
            while let Some((job, pending)) = rx.recv().await {
                let permit = limit.clone().acquire_owned().await.expect("semaphore is never closed");
                let ctx = ctx.clone();
                tokio::spawn(async move {
                    if let Err(e) = deliver(&ctx, job).await {
                        tracing::error!("deliver job: {e:#}");
                    }
                    drop((permit, pending));
                });
            }
        });
        Self {
            tx,
            pending: Default::default(),
        }
    }

    pub fn enqueue(&self, job: Job) -> Result<(), AppError> {
        check_size(&job.msg)?;
        {
            let mut counts = self.pending.lock().unwrap();
            let n = counts.entry(job.user_id).or_default();
            if *n >= MAX_PENDING_PER_USER {
                return Err(AppError::TooManyRequests("too many messages queued, slow down".into()));
            }
            *n += 1;
        }
        let pending = Pending {
            counts: self.pending.clone(),
            user_id: job.user_id,
        };
        self.tx.try_send((job, pending)).map_err(|_| AppError::Busy)
    }
}

fn check_size(msg: &Message) -> Result<(), AppError> {
    let len = |s: &Option<String>| s.as_ref().map_or(0, String::len);
    let too_long = |what: &str, max: usize| Err(AppError::BadRequest(format!("{what} is longer than {max} bytes")));
    if len(&msg.title) > MAX_TITLE {
        return too_long("title", MAX_TITLE);
    }
    if len(&msg.subtitle) > MAX_TITLE {
        return too_long("subtitle", MAX_TITLE);
    }
    if len(&msg.body) > MAX_BODY {
        return too_long("body", MAX_BODY);
    }
    if serde_json::to_vec(&msg.extras).map_or(0, |v| v.len()) > MAX_EXTRAS {
        return too_long("extra parameters", MAX_EXTRAS);
    }
    Ok(())
}

async fn deliver(ctx: &Arc<SendCtx>, job: Job) -> anyhow::Result<()> {
    let channels: Vec<Channel> = match &job.channel_key {
        Some(key) => {
            sqlx::query_as("SELECT * FROM channels WHERE user_id = ? AND key = ? AND active = 1")
                .bind(job.user_id)
                .bind(key)
                .fetch_all(&ctx.db)
                .await?
        }
        None => {
            sqlx::query_as("SELECT * FROM channels WHERE user_id = ? AND active = 1")
                .bind(job.user_id)
                .fetch_all(&ctx.db)
                .await?
        }
    };

    let msg = Arc::new(job.msg);
    let mut sends = JoinSet::new();
    for channel in channels {
        let (ctx, msg) = (ctx.clone(), msg.clone());
        sends.spawn(async move {
            let result = send_one(&ctx, &channel, &msg).await;
            if let Err(e) = &result {
                tracing::warn!(channel = channel.id, kind = %channel.kind, "send failed: {e:#}");
            }
            if let Err(e) = record_stat(&ctx, &channel.kind, result.is_ok()).await {
                tracing::error!("record send stat: {e:#}");
            }
        });
    }
    while sends.join_next().await.is_some() {}
    Ok(())
}

pub async fn send_one(ctx: &SendCtx, channel: &Channel, msg: &Message) -> anyhow::Result<()> {
    let kind = Kind::parse(&channel.kind).ok_or_else(|| anyhow::anyhow!("unknown channel kind {}", channel.kind))?;
    let config: serde_json::Value = serde_json::from_str(&channel.config)?;
    kind.send(ctx, channel.id, &config, msg).await
}

async fn record_stat(ctx: &SendCtx, kind: &str, ok: bool) -> sqlx::Result<()> {
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    sqlx::query(
        "INSERT INTO send_stats (date, kind, success, failed) VALUES (?, ?, ?, ?)
         ON CONFLICT(date, kind) DO UPDATE SET success = success + excluded.success, failed = failed + excluded.failed",
    )
    .bind(date)
    .bind(kind)
    .bind(ok as i64)
    .bind(!ok as i64)
    .execute(&ctx.db)
    .await?;
    Ok(())
}
