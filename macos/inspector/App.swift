import AppKit
import SwiftUI

@MainActor final class InspectorApp: NSObject, NSApplicationDelegate, NSWindowDelegate {
    var window: NSWindow!
    var model: ServiceModel!
    func applicationDidFinishLaunching(_ notification: Notification) {
        let bundle = Bundle.main
        let names = ["mouse", "mouse_mx_anywhere_3s", "mx_vertical"]
        let images = Dictionary(uniqueKeysWithValues: names.compactMap { name in bundle.url(forResource: name, withExtension: "png").flatMap { URL in NSImage(contentsOf: URL).map { (name, $0) } } })
        model = ServiceModel()
        let view = InspectorView(model: model, images: images)
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1120, height: 790), styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = "SuperLight"
        window.contentView = NSHostingView(rootView: view)
        window.minSize = NSSize(width: 980, height: 650)
        window.center()
        window.delegate = self
        window.isReleasedWhenClosed = false
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        model.start()
    }
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard model.dirty else { return true }
        let alert = NSAlert()
        alert.messageText = "Close with unsaved changes?"
        alert.informativeText = "The Rust service has not received these changes."
        alert.addButton(withTitle: "Keep editing")
        alert.addButton(withTitle: "Discard changes")
        return alert.runModal() == .alertSecondButtonReturn
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

@main struct Entry {
    @MainActor static func main() {
        let app = NSApplication.shared
        let delegate = InspectorApp()
        app.delegate = delegate
        app.setActivationPolicy(.regular)
        app.run()
        withExtendedLifetime(delegate) {}
    }
}
