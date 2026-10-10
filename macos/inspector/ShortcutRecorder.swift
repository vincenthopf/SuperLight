import SwiftUI
import AppKit

enum KeyShortcut {
    enum Capture: Equatable {
        case shortcut(String)
        case cancel
        case unsupported(String)
    }

    static let modifiers: [(flag: NSEvent.ModifierFlags, name: String)] = [
        (.control, "ctrl"), (.option, "alt"), (.shift, "shift"), (.command, "cmd")
    ]

    static let keyNames: [UInt16: String] = {
        var names: [UInt16: String] = [
            48: "tab", 49: "space", 36: "enter", 53: "esc", 51: "backspace", 117: "delete",
            123: "left", 124: "right", 125: "down", 126: "up",
            116: "pageup", 121: "pagedown", 115: "home", 119: "end", 33: "[", 30: "]"
        ]
        let letters: [UInt16] = [0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16, 6]
        for (index, code) in letters.enumerated() {
            names[code] = String(UnicodeScalar(UInt8(ascii: "a") + UInt8(index)))
        }
        for (digit, code) in [29, 18, 19, 20, 21, 23, 22, 26, 28, 25].enumerated() {
            names[UInt16(code)] = String(digit)
        }
        for (index, code) in [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111].enumerated() {
            names[UInt16(code)] = "f\(index + 1)"
        }
        return names
    }()

    private static let aliases = [
        "control": "ctrl", "option": "alt", "opt": "alt", "command": "cmd", "super": "cmd", "meta": "cmd",
        "win": "cmd", "windows": "cmd", "return": "enter", "escape": "esc"
    ]

    private static let symbols: [String: (glyph: String, spoken: String)] = [
        "ctrl": ("⌃", "Control"), "alt": ("⌥", "Option"), "shift": ("⇧", "Shift"), "cmd": ("⌘", "Command"),
        "tab": ("⇥", "Tab"), "space": ("Space", "Space"), "enter": ("↩", "Return"), "esc": ("⎋", "Escape"),
        "backspace": ("⌫", "Delete"), "delete": ("⌦", "Forward Delete"),
        "left": ("←", "Left Arrow"), "right": ("→", "Right Arrow"), "up": ("↑", "Up Arrow"), "down": ("↓", "Down Arrow"),
        "pageup": ("⇞", "Page Up"), "pagedown": ("⇟", "Page Down"), "home": ("↖", "Home"), "end": ("↘", "End"),
        "[": ("[", "Left Bracket"), "]": ("]", "Right Bracket")
    ]

    static let unsupportedHint = "Use a letter, digit, F1–F12, arrow, Space, Return, Tab, Escape, Delete, Page Up, Page Down, Home, End, [ or ]."

    static func modifierNames(_ flags: NSEvent.ModifierFlags) -> [String] {
        modifiers.filter { flags.contains($0.flag) }.map(\.name)
    }

    static func capture(_ event: NSEvent) -> Capture {
        let held = modifierNames(event.modifierFlags)
        if event.keyCode == 53 && held.isEmpty { return .cancel }
        guard let key = keyNames[event.keyCode] else {
            let character = (event.charactersIgnoringModifiers ?? "").trimmingCharacters(in: .whitespacesAndNewlines.union(.controlCharacters))
            let printable = character.count == 1 && !character.unicodeScalars.contains { (0xE000...0xF8FF).contains($0.value) }
            let keypad = event.modifierFlags.contains(.numericPad) ? "Keypad " : ""
            return .unsupported((printable ? keypad + "“\(character.uppercased())”" : "That key") + " can’t be used in a shortcut. " + unsupportedHint)
        }
        return .shortcut((held + [key]).joined(separator: "+"))
    }

    static func keys(_ combo: String) -> [String] {
        combo.split(separator: "+").map { $0.trimmingCharacters(in: .whitespaces).lowercased() }.filter { !$0.isEmpty }.map { aliases[$0] ?? $0 }
    }

    static func glyphs(_ combo: String) -> String {
        keys(combo).map { symbols[$0]?.glyph ?? $0.uppercased() }.joined()
    }

    static func spoken(_ combo: String) -> String {
        keys(combo).map { symbols[$0]?.spoken ?? $0.uppercased() }.joined(separator: " ")
    }
}

struct ShortcutRecorder: View {
    @Binding var shortcut: String
    @Binding var message: String
    var focus: FocusState<Bool>.Binding
    @State private var recording = false
    @State private var held = ""
    @State private var monitor: Any?
    @State private var releaseKey: UInt16?

    var body: some View {
        HStack(spacing: 10) {
            Text(display)
                .font(.title3.weight(.medium))
                .foregroundStyle(recording || shortcut.isEmpty ? .secondary : .primary)
                .lineLimit(1)
                .frame(maxWidth: .infinity, minHeight: 36, alignment: .leading)
                .padding(.horizontal, 10)
                .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 7))
                .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(recording ? Color.accentColor : Color.secondary.opacity(0.3), lineWidth: recording ? 2 : 1))
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("Keyboard shortcut")
                .accessibilityValue(recording ? "Recording. Press a key combination." : shortcut.isEmpty ? "None" : KeyShortcut.spoken(shortcut))
            Button(recording ? "Stop" : "Record") { recording ? stop() : start() }
                .accessibilityLabel(recording ? "Stop recording" : "Record shortcut")
                .focusable()
                .focused(focus)
                .onKeyPress(.space) {
                    start()
                    return .handled
                }
        }
        .onDisappear(perform: removeMonitor)
    }

    private var display: String {
        if recording { return held.isEmpty ? "Press a shortcut…" : KeyShortcut.glyphs(held) }
        return shortcut.isEmpty ? "No shortcut" : KeyShortcut.glyphs(shortcut)
    }

    private func start() {
        removeMonitor()
        message = ""
        held = ""
        recording = true
        announce("Recording. Press a key combination.")
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp, .flagsChanged]) { event in
            MainActor.assumeIsolated { consumes(event) } ? nil : event
        }
    }

    private func stop() {
        held = ""
        message = ""
        removeMonitor()
    }

    private func removeMonitor() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
        releaseKey = nil
        recording = false
    }

    private func consumes(_ event: NSEvent) -> Bool {
        guard recording else {
            if event.type == .keyUp && event.keyCode == releaseKey {
                removeMonitor()
                return true
            }
            if event.type == .keyDown && event.isARepeat && event.keyCode == releaseKey { return true }
            if event.type == .keyDown { removeMonitor() }
            return false
        }
        switch event.type {
        case .flagsChanged:
            held = KeyShortcut.modifierNames(event.modifierFlags).joined(separator: "+")
        case .keyDown:
            if event.isARepeat { return true }
            switch KeyShortcut.capture(event) {
            case .cancel:
                message = ""
                finish(event.keyCode)
                announce("Recording cancelled")
            case .shortcut(let value):
                shortcut = value
                message = ""
                finish(event.keyCode)
                announce("Recorded " + KeyShortcut.spoken(value))
            case .unsupported(let text):
                message = text
                announce(text)
            }
        default:
            break
        }
        return true
    }

    private func announce(_ text: String) {
        NSAccessibility.post(element: (NSApp.keyWindow ?? NSApp.mainWindow) as Any, notification: .announcementRequested, userInfo: [.announcement: text, .priority: NSAccessibilityPriorityLevel.high.rawValue])
    }

    private func finish(_ keyCode: UInt16) {
        recording = false
        held = ""
        releaseKey = keyCode
    }
}
