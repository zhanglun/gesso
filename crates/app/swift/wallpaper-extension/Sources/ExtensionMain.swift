// 扩展入口 + XPC 连接接受逻辑（对齐 Phosphene 的 WallpaperExtensionConfig）。
// M1：接受所有调用方（校验逻辑 M2 移植）；全部方法以桩应答。

import AppKit
import ExtensionFoundation
import Foundation
import ObjectiveC

struct WallpaperExtensionConfig: AppExtensionConfiguration {
    func accept(connection: NSXPCConnection) -> Bool {
        extLog("XPC 连接：宿主 PID=\(connection.processIdentifier)")

        let exported = NSXPCInterface(with: WallpaperExtensionXPCProtocol.self)

        // 私有 XPC 类型白名单：运行时从 dlopen 的 WallpaperExtensionKit 里取，
        // 缺失只记日志（不同 OS 版本字段可能增减，缺哪个补哪个）
        let typeNames = ["WallpaperIDXPC", "WallpaperCreationRequestXPC",
                         "WallpaperUpdateRequestXPC", "WallpaperRemoteContextXPC",
                         "WallpaperSnapshotXPC", "WallpaperContentTypeSetXPC",
                         "WallpaperChoiceIDXPC", "WallpaperChoiceIDsXPC",
                         "WallpaperExtensionChoiceRequestXPC",
                         "WallpaperChoiceRequestAdditionResultXPC",
                         "WallpaperDebugRequestXPC", "WallpaperDebugResponseXPC",
                         "WallpaperMigrationVersionXPC", "WallpaperSettingsViewModelsXPC",
                         "AuditTokenXPC"]
        let allTypes = NSMutableSet()
        for name in typeNames {
            if let cls = objc_getClass(name) { allTypes.add(cls) }
        }
        allTypes.add(NSString.self); allTypes.add(NSNumber.self); allTypes.add(NSData.self)
        allTypes.add(NSArray.self); allTypes.add(NSDictionary.self); allTypes.add(NSURL.self)
        allTypes.add(NSError.self); allTypes.add(NSDate.self)

        let classes = allTypes as! Set<AnyHashable>
        let s = WallpaperExtensionXPCProtocol.self
        let selectors: [(Selector, Int, Bool)] = [
            (#selector(s.acquire(withId:request:reply:)), 0, false),
            (#selector(s.acquire(withId:request:reply:)), 1, false),
            (#selector(s.acquire(withId:request:reply:)), 0, true),
            (#selector(s.update(withId:reply:)), 0, false),
            (#selector(s.invalidate(withId:reply:)), 0, false),
            (#selector(s.snapshot(withId:reply:)), 0, false),
            (#selector(s.snapshot(withId:reply:)), 0, true),
            (#selector(s.provideSettingsViewModels(withContentTypes:reply:)), 0, false),
            (#selector(s.provideSettingsViewModels(withContentTypes:reply:)), 0, true),
            (#selector(s.addChoice(request:onBehalfOf:reply:)), 0, false),
            (#selector(s.addChoice(request:onBehalfOf:reply:)), 1, false),
            (#selector(s.addChoice(request:onBehalfOf:reply:)), 0, true),
            (#selector(s.removeChoice(request:reply:)), 0, false),
            (#selector(s.selectedChoicesDidChange(for:reply:)), 0, false),
            (#selector(s.invokeContextMenuAction(withMenuItemID:groupItemID:reply:)), 0, false),
            (#selector(s.invokeContextMenuAction(withMenuItemID:groupItemID:reply:)), 1, false),
            (#selector(s.isChoiceDownloaded(with:reply:)), 0, false),
            (#selector(s.download(withChoiceID:reply:)), 0, false),
            (#selector(s.pauseDownload(for:reply:)), 0, false),
            (#selector(s.cancelDownload(for:reply:)), 0, false),
            (#selector(s.resumeDownload(for:reply:)), 0, false),
            (#selector(s.removeDownload(for:reply:)), 0, false),
            (#selector(s.migrateSelectedChoice(for:reply:)), 0, false),
            (#selector(s.migrateSelectedChoice(for:reply:)), 0, true),
            (#selector(s.migrate(from:to:reply:)), 0, false),
            (#selector(s.migrate(from:to:reply:)), 1, false),
            (#selector(s.skipShuffledContent(withId:reply:)), 0, false),
            (#selector(s.canSkipShuffledContent(withId:reply:)), 0, false),
            (#selector(s.handleDebug(request:reply:)), 0, false),
            (#selector(s.handleDebug(request:reply:)), 0, true),
            (#selector(s.handleNotification(withNamed:reply:)), 0, false),
        ]
        for (sel, idx, isReply) in selectors {
            exported.setClasses(classes, for: sel, argumentIndex: idx, ofReply: isReply)
        }

        connection.exportedInterface = exported
        connection.exportedObject = StubHandler(pid: connection.processIdentifier)
        connection.interruptionHandler = { extLog("XPC 中断") }
        connection.invalidationHandler = { extLog("XPC 断开") }
        connection.resume()
        extLog("XPC 接受（桩：全协议导出）")
        return true
    }
}

@main
final class GessoWallpaperExtension: NSObject, AppExtension {
    override required init() {
        super.init()
        extLog("扩展实例化：PID=\(ProcessInfo.processInfo.processIdentifier)")
        let fw = "/System/Library/PrivateFrameworks/WallpaperExtensionKit.framework/WallpaperExtensionKit"
        if dlopen(fw, RTLD_LAZY) != nil {
            extLog("WallpaperExtensionKit 加载成功")
            let critical = ["WallpaperRemoteContextXPC", "WallpaperSnapshotXPC",
                            "WallpaperCreationRequestXPC", "WallpaperIDXPC"]
            let missing = critical.filter { objc_getClass($0) == nil }
            extLog(missing.isEmpty
                   ? "运行时自检 OK（\(critical.count) 个关键类齐全）"
                   : "运行时自检：缺失 \(missing.joined(separator: ", "))")
        } else {
            extLog("WallpaperExtensionKit 加载失败：\(String(cString: dlerror()))")
        }
    }

    var configuration: some AppExtensionConfiguration {
        WallpaperExtensionConfig()
    }
}
