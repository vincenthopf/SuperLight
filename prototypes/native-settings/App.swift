import AppKit
import SwiftUI

@MainActor final class DesignApp: NSObject, NSApplicationDelegate, NSWindowDelegate {
    var windows: [NSWindow] = []
    var models: [PreviewModel] = []

    func applicationDidFinishLaunching(_ notification: Notification) {
        guard let imageURL = Bundle.main.url(forResource: "mouse", withExtension: "png"), let mouse = NSImage(contentsOf: imageURL) else {
            let alert = NSAlert()
            alert.messageText = "The bundled Mouser mouse image is missing. Rebuild the preview app."
            alert.runModal()
            NSApp.terminate(nil)
            return
        }
        let directory = Bundle.main.bundleURL.deletingLastPathComponent().appendingPathComponent("preview-state")
        let menu = NSMenu()
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "Quit SuperLight Designs", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        let appItem = NSMenuItem()
        appItem.submenu = appMenu
        menu.addItem(appItem)
        let windowMenu = NSMenu(title: "Window")
        let windowItem = NSMenuItem(title: "Window", action: nil, keyEquivalent: "")
        windowItem.submenu = windowMenu
        menu.addItem(windowItem)
        NSApp.mainMenu = menu
        NSApp.windowsMenu = windowMenu
        for (index, direction) in Direction.allCases.enumerated() {
            let model = PreviewModel(direction: direction, directory: directory)
            models.append(model)
            let size = direction == .inspector ? NSSize(width: 1100, height: 790) : NSSize(width: 1020, height: 790)
            let window = NSWindow(contentRect: NSRect(origin: .zero, size: size), styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
            window.title = "\(direction.number) · SuperLight — \(direction.rawValue)"
            window.contentView = NSHostingView(rootView: DesignView(model: model, mouseImage: mouse))
            window.minSize = NSSize(width: direction == .inspector ? 1020 : 930, height: 640)
            window.isReleasedWhenClosed = false
            window.delegate = self
            window.center()
            window.setFrameOrigin(NSPoint(x: window.frame.origin.x + CGFloat(index * 26), y: window.frame.origin.y - CGFloat(index * 24)))
            window.makeKeyAndOrderFront(nil)
            windows.append(window)
        }
        windows.first?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard let index = windows.firstIndex(of: sender), models[index].dirty else { return true }
        let alert = NSAlert()
        alert.messageText = "Close with unsaved preview changes?"
        alert.informativeText = "Only the saved preview will be restored next time. Your mouse is unaffected."
        alert.addButton(withTitle: "Keep editing")
        alert.addButton(withTitle: "Discard and close")
        return alert.runModal() == .alertSecondButtonReturn
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if models.contains(where: \.dirty) {
            let alert = NSAlert()
            alert.messageText = "Quit with unsaved preview changes?"
            alert.addButton(withTitle: "Keep editing")
            alert.addButton(withTitle: "Discard and quit")
            return alert.runModal() == .alertSecondButtonReturn ? .terminateNow : .terminateCancel
        }
        return .terminateNow
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

@main struct Entry {
    @MainActor static func main() {
        let app = NSApplication.shared
        let delegate = DesignApp()
        app.delegate = delegate
        app.setActivationPolicy(.regular)
        app.run()
        withExtendedLifetime(delegate) {}
    }
}
