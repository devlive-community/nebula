---
title: 用 Rust + Tauri 做一个多云对象存储管理器,聊聊三层解耦的架构
date: 2026-07-07
author: Nebula Team
description: 各家云的对象存储 SDK 在 Rust 生态里参差不齐,想做一个统一界面就得先把"厂商差异"关在一个笼子里。Nebula 用三层解耦把 SDK、抽象层和界面彻底分开。
tags: ['公告', '架构']
---

> 项目在这里:<https://github.com/devlive-community/nebula>。如果你也在用 Tauri 做桌面工具,或者被国内云的对象存储 SDK 折腾过,这篇也许能帮你少走点弯路。

## 为什么做 Nebula

对象存储人人都在用,但每家云的控制台都长得不一样,命令行工具又各说各话。我想要的很简单:**一个界面,管理我所有云上的对象存储**——浏览、上传下载、分享,像本地文件管理器一样顺手。

## 三层解耦

Nebula 的核心是把"厂商差异"关进最底层,让上层完全无感:

```
cloud-core  →  各厂商 SDK  →  provider 适配层  →  app
```

- **cloud-core**:签名原语(HMAC/SHA/base64)、HTTP 客户端、重试、分页等公共积木。
- **厂商 SDK**(如 `aliyun-oss`):从零手写,不依赖任何聚合库,可独立发布到 crates.io。
- **provider 适配层**:把某家 SDK 包成统一的 `StorageProvider` trait。
- **app**:Tauri + React,只认 `StorageProvider`,不知道也不关心底层是哪家云。

这样加一家新云,只是"写一个 SDK + 一个适配层 + 注册一下",**界面一行都不用改**。

## 签名是最硬的骨头

国内云的对象存储大多有自己的签名算法(OSS 是 HMAC-SHA1 + CanonicalizedResource)。签名写错就是一路 `SignatureDoesNotMatch`,而且很难 debug。我们的做法是:**用官方文档给的示例向量写单元测试**,逐字节比对,离线就能证明签名实现是对的;联网的部分再用真实账号跑一遍冒烟。

## 前后端边界

Tauri 的后端是 Rust,前端是 React。我们把业务逻辑放在一个框架无关的 `app-core` crate 里(可以 `cargo test`),Tauri command 只是极薄的一层桥接。这样绝大部分逻辑不依赖 GUI 就能测。

## 接下来

首发接入了阿里云 OSS。腾讯云 COS、华为云 OBS、AWS S3 等都可以按同一套《SDK 开发手册》复刻。欢迎来 [GitHub](https://github.com/devlive-community/nebula) 一起玩。
