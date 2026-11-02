// Prints the number of the first window on screen that belongs to the named app, if it has one.
import CoreGraphics
import Foundation

let owner = CommandLine.arguments[1]
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
for window in windows where window["kCGWindowOwnerName"] as? String == owner {
    if let number = window["kCGWindowNumber"] as? Int {
        print(number)
        break
    }
}
