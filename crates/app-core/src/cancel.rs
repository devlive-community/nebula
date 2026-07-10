//! 传输取消:把取消标志接到字节流上。
//!
//! 迁移的字节在 provider 的 `write_stream` 内部被消费,无法直接在循环里检查取消。
//! 这里给**源读取流**套一层:取消置位后,下一个分块变为错误、终止整条流,从而中断正在写入的
//! 目标端上传。上层再据取消标志把错误归一化为 [`AppError::Cancelled`](crate::AppError::Cancelled)。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures::StreamExt;
use nebula_provider::{ByteStream, ProviderError};

/// 给字节流套上取消检查。取消置位后,下一个分块变为错误,终止整条流。
pub(crate) fn cancellable(stream: ByteStream, cancel: Arc<AtomicBool>) -> ByteStream {
    Box::pin(stream.map(move |item| {
        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Backend("已取消".into()));
        }
        item
    }))
}
