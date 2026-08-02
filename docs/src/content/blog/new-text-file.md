---
title: 新建文本文件:不新建接口,把它当成一次普通上传
date: 2026-07-27
author: Nebula Team
description: 在当前目录新建一个文本文件,不需要给对象存储加"创建文件"这种概念——存储本身没有这个动作,写内容进去就是一次上传。这篇讲这个小对话框怎么复用已有的上传路径,以及顺手踩到的 Content-Type 坑。
tags: ['开发', '对象存储', '前端']
---

Nebula 一直能上传本地文件,但"就地敲几行内容存成一个新文件"(比如随手记一个 `readme.md` 或 `notes.txt`)之前得先在本地建好文件再拖上来。这次加一个「新建文本文件」对话框:填文件名、填内容,直接在当前目录生成对象。

## 对象存储没有"新建文件"这个动作

这是这个功能最值得说的一点:对象存储只有 `PUT`(写入一个 key 的内容),没有"创建空文件"再"编辑"这两步。所以「新建文本文件」在实现上**不是新功能**,只是给已有的上传路径喂了一份来自输入框而不是本地磁盘的字节:

```ts
const create = async () => {
  const dest = joinRemote(dir, name.trim());
  const bytes = Array.from(new TextEncoder().encode(content));
  await api.putImageBytes(account, dest, bytes, mimeFor(name));
  onCreated();
};
```

`putImageBytes` 这个名字是历史遗留(最早是给图片编辑器传字节用的),但它本质就是"传一段字节到某个 key",文本内容一样能用。

## Content-Type 不能留空

这里手动按扩展名给了一个 Content-Type:

```ts
const mimeFor = (fn: string) =>
  /\.md$/i.test(fn) ? "text/markdown; charset=utf-8" : "text/plain; charset=utf-8";
```

这么做是必要的——普通的"上传本地文件"路径当时还没有默认类型推断,不传 Content-Type 就会被对象存储回退成 `application/octet-stream`,新建的 `.md`/`.txt` 打开会变成下载而不是预览。这个对话框先手动把这一步补上了;后续上传路径统一加上按扩展名推断默认类型后,新建文本文件和普通上传现在走的是同一套推断逻辑。

## 小结

给对象存储加"新建 X 类型文件"的功能时,先想清楚它本质是不是一次换了数据来源的 `PUT`——这次是从"读本地磁盘"换成"读一个文本框",上传路径、进度、去重这些都不用重新实现,只需要把 Content-Type 这一步交代清楚。
