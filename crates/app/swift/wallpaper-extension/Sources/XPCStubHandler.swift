// M1 桩处理器：全协议导出，逐方法记日志。
// acquire/snapshot 回错误（不渲染——渲染是 M2）；枚举/查询类回良性默认值。

import Foundation

final class StubHandler: NSObject, WallpaperExtensionXPCProtocol {
    let connPID: Int32
    private static let err = NSError(domain: "dev.gesso.wallpaper.m1",
                                     code: 1,
                                     userInfo: [NSLocalizedDescriptionKey: "M1 桩：渲染未实现"])

    init(pid: Int32) { connPID = pid }

    func acquire(withId id: Any?, request: Any?, reply: @escaping (Any?, Error?) -> Void) {
        extLog("acquire 到达（宿主 PID=\(connPID)）——系统请求渲染壁纸 ✓")
        reply(nil, Self.err)
    }
    func update(withId id: Any?, reply: @escaping (Error?) -> Void) {
        extLog("update 到达"); reply(nil)
    }
    func invalidate(withId id: Any?, reply: @escaping (Error?) -> Void) {
        extLog("invalidate 到达"); reply(nil)
    }
    func snapshot(withId id: Any?, reply: @escaping (Any?, Error?) -> Void) {
        extLog("snapshot 到达（锁屏截图）"); reply(nil, Self.err)
    }
    func provideSettingsViewModels(withContentTypes types: Any?, reply: @escaping (Any?, Error?) -> Void) {
        extLog("provideSettingsViewModels 到达（系统设置要条目目录）")
        reply(nil, Self.err)
    }
    func addChoice(request: Any?, onBehalfOf process: Any?, reply: @escaping (Any?, Error?) -> Void) {
        extLog("addChoice 到达"); reply(nil, Self.err)
    }
    func removeChoice(request: Any?, reply: @escaping (Error?) -> Void) { reply(nil) }
    func selectedChoicesDidChange(for id: Any?, reply: @escaping (Error?) -> Void) { reply(nil) }
    func invokeContextMenuAction(withMenuItemID itemID: Any?, groupItemID: Any?, reply: @escaping (Error?) -> Void) { reply(nil) }
    func isChoiceDownloaded(with choiceID: Any?, reply: @escaping (NSNumber?, Error?) -> Void) {
        reply(NSNumber(value: true), nil)
    }
    func download(withChoiceID choiceID: Any?, reply: @escaping (Error?) -> Void) -> Any? {
        reply(nil)
        return nil
    }
    func pauseDownload(for choiceID: Any?, reply: @escaping (Error?) -> Void) { reply(nil) }
    func cancelDownload(for choiceID: Any?, reply: @escaping (Error?) -> Void) { reply(nil) }
    func resumeDownload(for choiceID: Any?, reply: @escaping (Error?) -> Void) { reply(nil) }
    func removeDownload(for choiceID: Any?, reply: @escaping (Error?) -> Void) { reply(nil) }
    func migrateSelectedChoice(for id: Any?, reply: @escaping (Error?) -> Void) { reply(nil) }
    func migrate(from src: Any?, to dst: Any?, reply: @escaping (Error?) -> Void) { reply(nil) }
    func skipShuffledContent(withId id: Any?, reply: @escaping (Error?) -> Void) { reply(nil) }
    func canSkipShuffledContent(withId id: Any?, reply: @escaping (Bool, Error?) -> Void) {
        reply(false, nil)
    }
    func handleDebug(request: Any?, reply: @escaping (Any?, Error?) -> Void) {
        extLog("debug 请求：\(String(describing: request))")
        reply(nil, Self.err)
    }
    func handleNotification(withNamed name: Any?, reply: @escaping (Error?) -> Void) { reply(nil) }
}
