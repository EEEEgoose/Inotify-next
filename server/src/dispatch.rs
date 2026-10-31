//! Send queue: requests enqueue a job and return; background tasks deliver it to every target
//! channel. A failing channel is logged and counted, and never stops the queue.

use crate::{
    channels::{Kind, Message, SendCtx},
    db::Channel,
    error::AppError,
};
use std::sync::Arc;
use tokio::{
    sync::{mpsc, Semaphore},
    task::JoinSet,
};

const QUEUE_CAPACITY: usize = 10_000;

#[derive(Debug)]
pub struct Job {
    pub user_id: i64,
    /// Only this channel; all of the user's active channels when `None`.
    pub channel_key: Option<String>,
    pub msg: Message,
}

#[derive(Clone)]
pub struct Dispatcher {
    tx: mpsc::Sender<Job>,
}

impl Dispatcher {
    pub fn start(ctx: Arc<SendCtx>, concurrency: usize) -> Self {
        let (tx, mut rx) = mpsc::channel::<Job>(QUEUE_CAPACITY);
        let limit = Arc::new(Semaphore::new(concurrency));
        tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                let permit = limit.clone().acquire_owned().await.expect("semaphore is never closed");
                let ctx = ctx.clone();
                tokio::spawn(async move {
                    if let Err(e) = deliver(&ctx, job).await {
                        tracing::error!("deliver job: {e:#}");
                    }
                    drop(permit);
                });
            }
        });
        Self { tx }
    }

    pub fn enqueue(&self, job: Job) -> Result<(), AppError> {
        self.tx.try_send(job).map_err(|_| AppError::Busy)
    }
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
