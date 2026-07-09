//! 用真实 OBS 账号做一次端到端冒烟验证:put → head → list → get → delete → multipart。
//!
//! 通过环境变量传入凭证(不要把密钥写进代码 / 提交进仓库):
//!
//! ```bash
//! export OBS_ACCESS_KEY=你的AK
//! export OBS_SECRET_KEY=你的SK
//! export OBS_ENDPOINT=obs.cn-north-4.myhuaweicloud.com   # 你 bucket 所在区域
//! export OBS_BUCKET=你的bucket名
//! cargo run -p huawei-obs --example huawei_smoke
//! ```
//!
//! 程序会在 bucket 里创建一个临时对象,验证完自动删除。

use std::env;

use futures::StreamExt;
use huawei_obs::ObsClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ak = env::var("OBS_ACCESS_KEY").expect("需要设置 OBS_ACCESS_KEY");
    let sk = env::var("OBS_SECRET_KEY").expect("需要设置 OBS_SECRET_KEY");
    let endpoint = env::var("OBS_ENDPOINT").expect("需要设置 OBS_ENDPOINT");
    let bucket = env::var("OBS_BUCKET").expect("需要设置 OBS_BUCKET");

    let client = ObsClient::new(ak, sk, endpoint);
    let key = "nebula-smoke-test.txt";
    let content = b"hello from nebula huawei-obs smoke test";

    println!("bucket = {bucket}, key = {key}\n");

    // 0) 列举账号下的 bucket(验证 service endpoint 签名路径)
    {
        let stream = client.list_buckets();
        futures::pin_mut!(stream);
        let mut names = Vec::new();
        while let Some(item) = stream.next().await {
            names.push(item?.name);
        }
        println!(
            "[0/5] list_buckets  ✅ 账号下 {} 个 bucket{}",
            names.len(),
            if names.iter().any(|n| n == &bucket) {
                ",含目标 bucket"
            } else {
                ""
            }
        );
    }

    // 1) 上传
    client
        .put_object(&bucket, key, content.to_vec(), Some("text/plain"))
        .await?;
    println!("[1/5] put_object    ✅ 已上传 {} 字节", content.len());

    // 2) 元信息
    let meta = client.head_object(&bucket, key).await?;
    println!(
        "[2/5] head_object   ✅ size={} type={:?} etag={:?}",
        meta.content_length, meta.content_type, meta.etag
    );

    // 3) 列举(打印前若干个)
    let stream = client.list_objects(&bucket, None);
    futures::pin_mut!(stream);
    let mut count = 0usize;
    let mut saw_key = false;
    while let Some(item) = stream.next().await {
        let obj = item?;
        if obj.key == key {
            saw_key = true;
        }
        if count < 5 {
            println!("        · {} ({} bytes)", obj.key, obj.size);
        }
        count += 1;
    }
    println!(
        "[3/5] list_objects  ✅ 共 {count} 个对象,{}包含刚上传的 key",
        if saw_key { "" } else { "⚠️ 未" }
    );

    // 4) 下载并校验内容
    let got = client.get_object(&bucket, key).await?;
    assert_eq!(got.as_ref(), content, "下载内容与上传不一致!");
    println!("[4/5] get_object    ✅ 内容与上传一致");

    // 5) 删除
    client.delete_object(&bucket, key).await?;
    println!("[5/5] delete_object ✅ 已清理临时对象");

    // 6) 分片上传(验证子资源签名路径):造 250KB 数据,强制切成多片
    let mp_key = "nebula-smoke-multipart.bin";
    let part_size = huawei_obs::multipart::MIN_PART_SIZE; // 100KB
    let big: Vec<u8> = (0..(part_size * 2 + 12345))
        .map(|i| (i % 251) as u8)
        .collect();
    let big_len = big.len();
    client
        .upload_multipart(
            &bucket,
            mp_key,
            big.clone(),
            part_size,
            Some("application/octet-stream"),
        )
        .await?;
    let back = client.get_object(&bucket, mp_key).await?;
    assert_eq!(back.len(), big_len, "分片上传后大小不一致");
    assert_eq!(back.as_ref(), big.as_slice(), "分片上传后内容不一致");
    client.delete_object(&bucket, mp_key).await?;
    println!(
        "[6/6] multipart     ✅ {big_len} 字节分 {} 片上传/校验/清理完成",
        big_len.div_ceil(part_size)
    );

    println!("\n🎉 全链路验证通过");
    Ok(())
}
