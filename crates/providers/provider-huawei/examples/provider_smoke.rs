//! 通过统一的 `dyn StorageProvider` 驱动华为云适配器,证明 SDK 藏在抽象层后也能跑通。
//!
//! ```bash
//! export OBS_ACCESS_KEY=你的AK
//! export OBS_SECRET_KEY=你的SK
//! export OBS_ENDPOINT=obs.cn-north-4.myhuaweicloud.com
//! export OBS_BUCKET=你的bucket名
//! cargo run -p provider-huawei --example provider_smoke
//! ```

use std::env;
use std::sync::Arc;

use bytes::Bytes;
use nebula_provider::{ProviderRegistry, StorageProvider};
use provider_huawei::HuaweiProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ak = env::var("OBS_ACCESS_KEY").expect("需要设置 OBS_ACCESS_KEY");
    let sk = env::var("OBS_SECRET_KEY").expect("需要设置 OBS_SECRET_KEY");
    let endpoint = env::var("OBS_ENDPOINT").expect("需要设置 OBS_ENDPOINT");
    let bucket = env::var("OBS_BUCKET").expect("需要设置 OBS_BUCKET");

    // 像 App 一样:构造适配器,放进注册表,之后只用 dyn StorageProvider。
    let registry = ProviderRegistry::new();
    registry.register(Arc::new(HuaweiProvider::new(
        "huawei-main",
        ak,
        sk,
        endpoint,
    )));

    let provider = registry.get("huawei-main").expect("provider 已注册");
    let provider: &dyn StorageProvider = provider.as_ref();

    println!(
        "provider id = {}, caps = {:?}\n",
        provider.id(),
        provider.capabilities()
    );

    // 1) 列出 bucket(根)
    let buckets = provider.list("").await?;
    println!(
        "[1/5] list(\"\")        ✅ {} 个 bucket(目录)",
        buckets.len()
    );

    // 1b) 列出目标桶根一层(验证 delimiter 折叠:目录 vs 文件)
    let root = provider.list(&format!("{bucket}/")).await?;
    let dirs = root.iter().filter(|e| e.is_dir()).count();
    let files = root.len() - dirs;
    println!("      · {bucket}/ 顶层:{dirs} 个目录,{files} 个文件");

    // 2) 写入对象
    let path = format!("{bucket}/nebula-provider-smoke.txt");
    let content = Bytes::from_static(b"hello via unified StorageProvider");
    provider
        .write(&path, content.clone(), Some("text/plain"))
        .await?;
    println!("[2/5] write({path})  ✅ 已上传");

    // 3) stat
    let meta = provider.stat(&path).await?;
    println!(
        "[3/5] stat             ✅ name={} size={} dir={}",
        meta.name,
        meta.size,
        meta.is_dir()
    );

    // 4) read 校验
    let got = provider.read(&path).await?;
    assert_eq!(got, content, "读回内容与写入不一致");
    println!("[4/5] read             ✅ 内容一致");

    // 5) delete
    provider.delete(&path).await?;
    println!("[5/5] delete           ✅ 已清理");

    println!("\n🎉 通过统一 StorageProvider 的全链路验证通过");
    Ok(())
}
