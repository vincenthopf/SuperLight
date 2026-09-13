import AppKit
import SwiftUI

@MainActor final class InspectorApp: NSObject, NSApplicationDelegate, NSWindowDelegate {
    var window: NSWindow!
    var model: ServiceModel!
    var timer: Timer?
    func applicationDidFinishLaunching(_ notification: Notification) {
        let current = NSRunningApplication.current
        if let other = NSWorkspace.shared.runningApplications.first(where: { $0.processIdentifier != current.processIdentifier && $0.executableURL == current.executableURL }) {
            other.activate(options: [.activateAllWindows])
            NSApp.terminate(nil)
            return
        }
        let names = ["mouse", "mouse_mx_anywhere_3s", "mx_vertical"]
        let resources = URL(fileURLWithPath: CommandLine.arguments[0]).deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("Resources")
        let images = Dictionary(uniqueKeysWithValues: names.compactMap { name in NSImage(contentsOf: resources.appendingPathComponent(name + ".png")).map { (name, $0) } })
        model = ServiceModel()
        let view = InspectorView(model: model, images: images)
        NSWindow.allowsAutomaticWindowTabbing = false
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1120, height: 790), styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView], backing: .buffered, defer: false)
        window.title = "SuperLight"
        window.titlebarAppearsTransparent = true
        window.toolbarStyle = .unified
        window.setFrameAutosaveName("SuperLight.Inspector")
        window.contentView = NSHostingView(rootView: view)
        window.minSize = NSSize(width: 920, height: 650)
        if !window.setFrameUsingName("SuperLight.Inspector") { window.center() }
        window.delegate = self
        window.isReleasedWhenClosed = false
        window.makeKeyAndOrderFront(nil)
        configureMenus()
        NSApp.activate(ignoringOtherApps: true)
        model.start()
        updateRefreshTimer()
    }
    func configureMenus() {
        let menu = NSMenu()
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "About SuperLight", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Quit Settings", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        let appItem = NSMenuItem(); appItem.submenu = appMenu; menu.addItem(appItem)
        let edit = NSMenu(title: "Edit")
        for (title, selector, key) in [("Undo", "undo:", "z"), ("Cut", "cut:", "x"), ("Copy", "copy:", "c"), ("Paste", "paste:", "v"), ("Select All", "selectAll:", "a")] {
            edit.addItem(withTitle: title, action: Selector(selector), keyEquivalent: key)
        }
        let editItem = NSMenuItem(title: "Edit", action: nil, keyEquivalent: ""); editItem.submenu = edit; menu.addItem(editItem)
        NSApp.mainMenu = menu
    }
    func mayClose() -> Bool {
        guard let model else { return true }
        if model.saving { return false }
        guard model.dirty else { return true }
        let alert = NSAlert()
        alert.messageText = "Close with unsaved changes?"
        alert.informativeText = "The Rust service has not received these changes."
        alert.addButton(withTitle: "Keep editing")
        alert.addButton(withTitle: "Discard changes")
        return alert.runModal() == .alertSecondButtonReturn
    }
    func windowShouldClose(_ sender: NSWindow) -> Bool { mayClose() }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply { window == nil || !window.isVisible || mayClose() ? .terminateNow : .terminateCancel }
    func updateRefreshTimer() {
        guard window != nil, model != nil else { return }
        guard window.isVisible, !window.isMiniaturized, !NSApp.isHidden, window.occlusionState.contains(.visible) else {
            timer?.invalidate()
            timer = nil
            return
        }
        guard timer == nil else { return }
        model.refresh()
        timer = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.model.refresh() }
        }
    }
    func windowDidChangeOcclusionState(_ notification: Notification) { updateRefreshTimer() }
    func windowDidMiniaturize(_ notification: Notification) { updateRefreshTimer() }
    func windowDidDeminiaturize(_ notification: Notification) { updateRefreshTimer() }
    func applicationDidHide(_ notification: Notification) { updateRefreshTimer() }
    func applicationDidUnhide(_ notification: Notification) { updateRefreshTimer() }
    func applicationDidBecomeActive(_ notification: Notification) { updateRefreshTimer() }
    func applicationWillTerminate(_ notification: Notification) { timer?.invalidate() }
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
