//! 通过统一的 `dyn StorageProvider` 驱动 AWS S3 适配器,证明 SDK 藏在抽象层后也能跑通。
//!
//! ```bash
//! export AWS_ACCESS_KEY=你的AK
//! export AWS_SECRET_KEY=你的SK
//! export AWS_ENDPOINT=s3.us-east-1.amazonaws.com
//! export AWS_BUCKET=你的bucket名
//! cargo run -p provider-aws --example aws_provider_smoke
//! ```

use std::env;
use std::sync::Arc;

use bytes::Bytes;
use nebula_provider::{ProviderRegistry, StorageProvider};
use provider_aws::AwsProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ak = env::var("AWS_ACCESS_KEY").expect("需要设置 AWS_ACCESS_KEY");
    let sk = env::var("AWS_SECRET_KEY").expect("需要设置 AWS_SECRET_KEY");
    let endpoint = env::var("AWS_ENDPOINT").expect("需要设置 AWS_ENDPOINT");
    let bucket = env::var("AWS_BUCKET").expect("需要设置 AWS_BUCKET");

    let registry = ProviderRegistry::new();
    registry.register(Arc::new(AwsProvider::new("aws-main", ak, sk, endpoint)));

    let provider = registry.get("aws-main").expect("provider 已注册");
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
