---
title: 启动 App 连弹 N 次钥匙串密码:一次 macOS Keychain 的排查复盘
date: 2026-07-10
author: Nebula Team
description: 用户配了 4 个云账号,一开 App 系统就连着弹 4 次钥匙串授权。这背后其实是两个独立的问题——一个能靠代码根治,另一个是 macOS 把"始终允许"绑在了代码签名上。这是我们的排查与修复复盘。
tags: ['开发', 'macOS', 'Keychain', '安全']
---

Nebula 的一条铁律是:**AccessKeySecret 绝不落 SQLite**。数据库里只存账号的公开信息(endpoint、AccessKeyId、备注),真正的密钥交给操作系统的安全存储——macOS 是 Keychain,Windows 是凭据管理器,Linux 是内核 keyutils。这层抽象叫 `SecretStore`。

方向没错,但有用户反馈了一个体验很差的现象:

> 我配置了几个账号,启动 App 的时候系统钥匙串就要让我输入几次密码,这是什么问题?

配了 4 个账号,启动就弹 4 次。而且——**每次启动都弹**。拆开看,这其实是**两个独立的问题**,一个能靠代码根治,另一个不行。

## 问题一:N 个账号 = N 条钥匙串条目 = N 次弹窗

最初的实现很直白:一个账号一条钥匙串条目,account id 作为条目名。

```rust
pub struct KeyringSecrets {
    service: String,
}

impl KeyringSecrets {
    fn entry(&self, account: &str) -> Result<keyring::Entry> {
        Ok(keyring::Entry::new(&self.service, account)?)
    }
}

impl SecretStore for KeyringSecrets {
    fn get(&self, account: &str) -> Result<String> {
        Ok(self.entry(account)?.get_password()?)   // 每个账号读一次
    }
    // set / delete 同理,各自 new(service, account)
}
```

App 启动时会遍历所有已保存账号,给每个建 provider——每建一个就 `secrets.get(id)` 读一次密钥。**每次读都是对一条独立钥匙串条目的一次访问**,而 macOS 对"某个 App 想读某条钥匙串项"是逐条弹窗授权的。于是 4 个账号 = 4 条条目 = 4 次独立授权 = 4 次弹窗。

这个是纯粹的**存储结构问题**,可以根治。

## 修复:把 N 条合并成 1 条

思路很简单:所有账号的密钥合并成一个 `HashMap<String, String>`,序列化成 JSON,只存进**同一条**钥匙串条目;首次读取后缓存在内存里,整个进程只真正碰一次钥匙串。

```rust
const BLOB_KEY: &str = "__nebula_secrets__";

pub struct KeyringSecrets {
    service: String,
    /// 内存缓存;None 表示尚未从钥匙串加载
    cache: Mutex<Option<HashMap<String, String>>>,
}

impl KeyringSecrets {
    /// 确保合并条目已加载进缓存。整个进程只真正读钥匙串一次。
    fn load(&self) -> Result<MutexGuard<'_, Option<HashMap<String, String>>>> {
        let mut guard = self.cache.lock().unwrap();
        if guard.is_none() {
            let map = match self.blob_entry()?.get_password() {
                Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
                Err(keyring::Error::NoEntry) => HashMap::new(),
                Err(e) => return Err(e.into()),
            };
            *guard = Some(map);
        }
        Ok(guard)
    }
}
```

`get` 从缓存里取,`set` / `delete` 改完缓存再整体写回那一条 blob。这样无论配了几个账号,启动只读钥匙串**一次** → macOS 只弹**一次**。

### 别忘了老用户:读时自动迁移

已经在老版本里存过密钥的用户,他们的密钥还散在旧的"每账号一条"条目里。不能让他们升级后账号全丢。所以 `get` 里加一段回退迁移:合并条目里没有 → 去读旧的单账号条目,读到就搬进合并条目、删掉旧条目。

```rust
fn get(&self, account: &str) -> Result<String> {
    let mut guard = self.load()?;
    let map = guard.as_mut().unwrap();
    if let Some(s) = map.get(account) {
        return Ok(s.clone());
    }
    // 迁移:回退读旧的"每账号一条",读到就搬进合并条目并删掉旧的
    match self.legacy_entry(account)?.get_password() {
        Ok(secret) => {
            map.insert(account.to_string(), secret.clone());
            self.persist(map)?;
            let _ = self.legacy_entry(account)?.delete_credential();
            Ok(secret)
        }
        Err(keyring::Error::NoEntry) => Err(keyring::Error::NoEntry.into()),
        Err(e) => Err(e.into()),
    }
}
```

迁移是**惰性**的:哪个账号被读到就迁哪个,不需要一次性扫描,也不需要知道"历史上到底存过哪些"。升级后第一次启动会把老条目逐个搬进合并条目,之后就永远只有一条了。

## 问题二:为什么"每次启动"都还弹一次?

合并之后,弹窗从 N 次降到了 1 次——但**每次启动**仍然弹这 1 次,点了"始终允许"也不管用。这就不是代码能解决的了,得理解 macOS Keychain 的授权模型。

Keychain 的每条项都挂着一个 **ACL(访问控制列表)**,记录"哪些程序可以免密访问我"。你点"始终允许"时,系统把**当前这个 App**加进这条项的 ACL。关键在于:ACL 里记的不是文件路径,而是 App 的**代码签名标识(designated requirement)**。

- 一个**签名稳定**的 App(用固定证书签过名),每次启动签名都一样 → 命中 ACL → 免密,不再弹。
- 一个**未签名 / 每次重新构建**的开发版,`cargo tauri build` 出来的二进制每次内容都变,ad-hoc 签名也跟着变 → macOS 认为这是个"新程序",上次的授权作废 → 每次启动都重新弹。

也就是说,开发期天天重编译、天天换签名,这个弹窗**结构上就消不掉**。它衡量的根本不是"你这次读了几条",而是"你是不是我认识的那个 App"。

## 结论:两层问题,两种解法

| 现象 | 根因 | 能否根治 |
|------|------|----------|
| 一次启动弹 **N** 次 | N 个账号 = N 条钥匙串条目 = N 次授权 | ✅ 合并成 1 条 blob + 内存缓存,已修复 |
| **每次**启动都弹 1 次 | "始终允许"绑定代码签名,开发版签名每次都变 | ⚠️ 需要**稳定签名**,非代码问题 |

第二层要彻底消除,发布正式版时用固定的 **Apple Developer ID** 证书签名 + 公证,用户点一次"始终允许"就永久生效。没有付费开发者账号也有免费的过渡办法:自己生成一张**固定的自签名代码签名证书**,每次构建都用它签——签名稳定了,"始终允许"就能记住(代价是 Gatekeeper 仍会提示"未识别的开发者",右键打开一次即可)。

## 一点收获

排查这类问题,最容易掉进的坑是把两层现象当成一个 bug 去修。"弹窗太多"听起来是一件事,但"弹 N 次"和"每次都弹"的成因完全正交:前者是**你的存储结构**,后者是**操作系统的信任模型**。分清楚之后,才知道哪部分该写代码、哪部分该去配签名——把力气花在能根治的那半边,另一半老老实实交给发布流程。
