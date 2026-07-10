//! 通过统一的 `dyn StorageProvider` 驱动腾讯云 COS 适配器。
//!
//! ```bash
//! export COS_SECRET_ID=你的SecretId
//! export COS_SECRET_KEY=你的SecretKey
//! export COS_ENDPOINT=cos.ap-beijing.myqcloud.com
//! export COS_BUCKET=你的bucket名(含 appid)
//! cargo run -p provider-tencent --example tencent_provider_smoke
//! ```

use std::env;
use std::sync::Arc;

use bytes::Bytes;
use nebula_provider::{ProviderRegistry, StorageProvider};
use provider_tencent::TencentProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sid = env::var("COS_SECRET_ID").expect("需要设置 COS_SECRET_ID");
    let sk = env::var("COS_SECRET_KEY").expect("需要设置 COS_SECRET_KEY");
    let endpoint = env::var("COS_ENDPOINT").expect("需要设置 COS_ENDPOINT");
    let bucket = env::var("COS_BUCKET").expect("需要设置 COS_BUCKET");

    let registry = ProviderRegistry::new();
    registry.register(Arc::new(TencentProvider::new(
        "cos-main", sid, sk, endpoint,
    )));

    let provider = registry.get("cos-main").expect("provider 已注册");
    let provider: &dyn StorageProvider = provider.as_ref();

    let buckets = provider.list("").await?;
    println!("[1/4] list(\"\")   ✅ {} 个 bucket", buckets.len());

    let path = format!("{bucket}/nebula-provider-smoke.txt");
    let content = Bytes::from_static(b"hello via unified StorageProvider");
    provider
        .write(&path, content.clone(), Some("text/plain"))
        .await?;
    println!("[2/4] write      ✅ 已上传");

    let got = provider.read(&path).await?;
    assert_eq!(got, content, "读回内容与写入不一致");
    println!("[3/4] read       ✅ 内容一致");

    provider.delete(&path).await?;
    println!("[4/4] delete     ✅ 已清理");

    println!("\n🎉 通过统一 StorageProvider 的全链路验证通过");
    Ok(())
}
