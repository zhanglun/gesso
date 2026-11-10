// Gesso 壁纸扩展（路线 A，M1）—— 日志工具
// 器材与契约来源：Phosphene（kageroumado/phosphene，MIT）+ 本机 spike
// （~/spike-wallpaper-extension-20261109/）；设计见技术方案 §10.6。

import Foundation

func extLog(_ s: String) {
    let line = "[\(Date())] \(s)\n"
    let path = "/tmp/gesso-wallpaper-ext.log"
    if let fh = FileHandle(forWritingAtPath: path) {
        fh.seekToEndOfFile()
        fh.write(line.data(using: .utf8)!)
        fh.closeFile()
    } else {
        try? line.write(toFile: path, atomically: true, encoding: .utf8)
    }
}
