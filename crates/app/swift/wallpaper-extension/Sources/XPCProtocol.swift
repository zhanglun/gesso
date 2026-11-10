// 宿主（WallpaperAgent）→ 扩展 的 XPC 协议声明。
// 选择器名必须与 WallpaperAgent 的 ObjC 调用逐字一致；参数/返回类型对照
// Phosphene 的 WallpaperExtension-Bridging-Header.h（私有框架无公开头文件，
// 该头文件即事实契约）。

import Foundation

@objc protocol WallpaperExtensionXPCProtocol: NSObjectProtocol {
    // 生命周期
    @objc(acquireWithId:request:reply:)
    func acquire(withId id: Any?, request: Any?, reply: @escaping (Any?, Error?) -> Void)
    @objc(updateWithId:reply:)
    func update(withId id: Any?, reply: @escaping (Error?) -> Void)
    @objc(invalidateWithId:reply:)
    func invalidate(withId id: Any?, reply: @escaping (Error?) -> Void)
    @objc(snapshotWithId:reply:)
    func snapshot(withId id: Any?, reply: @escaping (Any?, Error?) -> Void)

    // 设置（系统设置 → 壁纸 分区的数据来源）
    @objc(provideSettingsViewModelsWithContentTypes:reply:)
    func provideSettingsViewModels(withContentTypes types: Any?, reply: @escaping (Any?, Error?) -> Void)

    // 内容目录（Choices）
    @objc(addChoiceRequestWithChoiceRequest:onBehalfOfProcess:reply:)
    func addChoice(request: Any?, onBehalfOf process: Any?, reply: @escaping (Any?, Error?) -> Void)
    @objc(removeChoiceRequestWithChoiceRequest:reply:)
    func removeChoice(request: Any?, reply: @escaping (Error?) -> Void)
    @objc(selectedChoicesDidChangeFor:reply:)
    func selectedChoicesDidChange(for id: Any?, reply: @escaping (Error?) -> Void)
    @objc(invokeContextMenuActionWithMenuItemID:groupItemID:reply:)
    func invokeContextMenuAction(withMenuItemID itemID: Any?, groupItemID: Any?, reply: @escaping (Error?) -> Void)

    // 下载（isChoiceDownloaded 的 reply 携带 NSNumber 装箱 Bool；声明成裸 BOOL
    // 会让 NSXPC 以消息不可解码为由取消整条连接——Phosphene 注释原话）
    @objc(isChoiceDownloadedWith:reply:)
    func isChoiceDownloaded(with choiceID: Any?, reply: @escaping (NSNumber?, Error?) -> Void)
    @objc(downloadWithChoiceID:reply:)
    func download(withChoiceID choiceID: Any?, reply: @escaping (Error?) -> Void) -> Any?
    @objc(pauseDownloadFor:reply:)
    func pauseDownload(for choiceID: Any?, reply: @escaping (Error?) -> Void)
    @objc(cancelDownloadFor:reply:)
    func cancelDownload(for choiceID: Any?, reply: @escaping (Error?) -> Void)
    @objc(resumeDownloadFor:reply:)
    func resumeDownload(for choiceID: Any?, reply: @escaping (Error?) -> Void)
    @objc(removeDownloadFor:reply:)
    func removeDownload(for choiceID: Any?, reply: @escaping (Error?) -> Void)

    // 迁移
    @objc(migrateSelectedChoiceFor:reply:)
    func migrateSelectedChoice(for id: Any?, reply: @escaping (Error?) -> Void)
    @objc(migrateFrom:to:reply:)
    func migrate(from src: Any?, to dst: Any?, reply: @escaping (Error?) -> Void)

    // 轮换
    @objc(skipShuffledContentWithId:reply:)
    func skipShuffledContent(withId id: Any?, reply: @escaping (Error?) -> Void)
    @objc(canSkipShuffledContentWithId:reply:)
    func canSkipShuffledContent(withId id: Any?, reply: @escaping (Bool, Error?) -> Void)

    // 调试与通知
    @objc(handleDebugRequestFor:reply:)
    func handleDebug(request: Any?, reply: @escaping (Any?, Error?) -> Void)
    @objc(handleNotificationWithNamed:reply:)
    func handleNotification(withNamed name: Any?, reply: @escaping (Error?) -> Void)
}

// 扩展 → 宿主方向的代理协议（M1 只声明不使用，保契约完整）
@objc protocol WallpaperExtensionProxyXPCProtocol: NSObjectProtocol {
    @objc(pingWithId:)
    func ping(with id: Any?)
    @objc(updateSettingsViewModels:reply:)
    func updateSettingsViewModels(_ models: Any?, reply: @escaping (Error?) -> Void)
    @objc(requestReadOnlyAccessTo:reply:)
    func requestReadOnlyAccess(to url: Any?, reply: @escaping (Any?) -> Void)
    @objc(invalidateSnapshotsWithReply:)
    func invalidateSnapshots(reply: @escaping (Error?) -> Void)
}
