import SwiftUI
import AppKit

let controlNames = ["Middle click", "Thumb gesture", "Back", "Forward", "Scroll left", "Scroll right", "Wheel mode", "Gesture left", "Gesture right", "Gesture up", "Gesture down", "DPI button"]
let controlKeys = ["middle", "gesture", "xbutton1", "xbutton2", "hscroll_left", "hscroll_right", "mode_shift", "gesture_left", "gesture_right", "gesture_up", "gesture_down", "dpi_switch"]

enum Page: String, CaseIterable {
    case buttons = "Buttons", scroll = "Point & scroll", profiles = "Profiles", settings = "Settings"
    var icon: String { switch self {
    case .buttons: "computermouse"
    case .scroll: "arrow.up.arrow.down"
    case .profiles: "square.stack"
    case .settings: "slider.horizontal.3"
    } }
}

struct Profile: Equatable, Identifiable {
    var id: String
    var name: String
    var application: String
    var actions: [String]
}

struct SettingsData: Equatable {
    var profiles: [Profile] = []
    var dpi = 1000.0
    var verticalInverted = false
    var horizontalInverted = false
    var smartShift = false
    var threshold = 25.0
    var startAtLogin = false
    var ignoreTrackpad = true
    var wheelMode = "ratchet"
    var presets = "800, 1200, 1600, 2400"
    var gestureThreshold = 50.0
    var gestureDeadzone = 40.0
    var gestureTimeout = 3000.0
    var gestureCooldown = 500.0
}

struct ServiceFailure: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

enum NativeBridge {
    static func call(_ request: [String: Any]) throws -> [String: Any] {
        let bytes = try JSONSerialization.data(withJSONObject: request)
        let pointer = bytes.withUnsafeBytes { buffer in superlight_call(buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count) }
        guard let pointer else { throw ServiceFailure(message: "The native bridge returned no response.") }
        defer { superlight_free(pointer) }
        let response = Data(bytes: pointer, count: strlen(pointer))
        guard let value = try JSONSerialization.jsonObject(with: response) as? [String: Any] else { throw ServiceFailure(message: "Malformed service response.") }
        guard value["ok"] as? Bool == true else { throw ServiceFailure(message: value["error"] as? String ?? "The service rejected this operation.") }
        return value
    }
}

@MainActor final class ServiceModel: ObservableObject {
    @Published var data = SettingsData()
    @Published var baseline = SettingsData()
    @Published var page = Page.buttons
    @Published var selected = 0
    @Published var profile = 0
    @Published var theme = "System"
    @Published var message = "Connecting to the service…"
    @Published var error: String?
    @Published var connected = false
    @Published var saving = false
    @Published var busy = false
    @Published var conflicted = false
    @Published var snapshot: [String: Any] = [:]
    @Published var actions: [[String]] = []
    private var baselineConfig: [String: Any] = [:]
    private var revision: UInt64?
    private var instance: String?
    private var baselineTheme = "System"
    private let queue = DispatchQueue(label: "io.superlight.settings-ipc", qos: .userInitiated)
    var serviceProcess: Process?
    private var attemptedStart = false

    var dirty: Bool { data != baseline || theme != baselineTheme }
    var scheme: ColorScheme? { theme == "System" ? nil : theme == "Dark" ? .dark : .light }
    var action: String { data.profiles.isEmpty ? "Pass through" : data.profiles[profile].actions[selected] }
    var device: [String: Any] { snapshot["device"] as? [String: Any] ?? [:] }
    var deviceName: String { device["name"] as? String ?? "SuperLight" }
    var deviceDetected: Bool { !device.isEmpty }
    var connectionSummary: String {
        if !connected { return "Reconnecting to service…" }
        if !deviceDetected { return "Searching for a Logitech mouse…" }
        return "\(deviceName) · \(device["transport"] as? String ?? "Connected")"
    }
    var permission: [String: Any] { snapshot["permissions"] as? [String: Any] ?? [:] }
    var dpiBounds: ClosedRange<Double> { Double(device["dpi_min"] as? Int ?? 200)...Double(max(device["dpi_max"] as? Int ?? 8000, device["dpi_min"] as? Int ?? 200)) }
    var canSave: Bool { connected && dirty && !conflicted && !busy && !data.profiles.isEmpty }
    var status: String {
        if !connected { return "Service unavailable" }
        if snapshot["paused"] as? Bool == true { return "Remapping paused" }
        if snapshot["suspended"] as? Bool == true { return "Remapping suspended" }
        if permission["listen"] as? Bool != true || permission["inject"] as? Bool != true { return "Input permissions required" }
        if device.isEmpty { return "No Logitech mouse connected" }
        if snapshot["native_ready"] as? Bool != true { return "Input unavailable" }
        if snapshot["hardware_pending"] as? Bool == true { return "Applying mouse settings…" }
        return "Mouse ready"
    }
    var actionGroups: [(String, [String])] {
        [("Actions", actions.map { $0[1] })]
    }
    func supports(_ capability: String) -> Bool { device[capability] as? Bool == true }
    func supportedButton(_ index: Int) -> Bool {
        if index == 1 || (7...10).contains(index) { return supports("supports_gesture") }
        if index == 6 { return supports("supports_mode_shift") }
        if index == 11 { return supports("supports_dpi_switch") }
        return true
    }
    func label(_ id: String) -> String { actions.first(where: { $0[0] == id })?[1] ?? id }
    func identifier(_ label: String) -> String { actions.first(where: { $0[1] == label })?[0] ?? label }

    func start() {
        request(["local": "catalog"]) { response in
            self.actions = response["actions"] as? [[String]] ?? []
            self.refresh()
        }
    }
    func refresh() { if !busy { command("get") } }
    func command(_ command: String, value: Bool? = nil) {
        var payload: [String: Any] = ["command": command]
        if let value { payload["value"] = value }
        request(["request": payload]) { response in self.accept(response) }
    }
    private func request(_ request: [String: Any], completion: @escaping ([String: Any]) -> Void) {
        guard !busy else { return }
        busy = true
        queue.async {
            let result = Result { try NativeBridge.call(request) }
            DispatchQueue.main.async {
                self.busy = false
                switch result {
                case .success(let response): completion(response)
                case .failure(let failure):
                    if request["local"] == nil { self.connected = false }
                    self.message = failure.localizedDescription
                    if self.saving || (request["request"] as? [String: Any])?["command"] as? String != "get" { self.error = failure.localizedDescription }
                    self.saving = false
                    if (request["request"] as? [String: Any])?["command"] as? String == "get" && !self.attemptedStart {
                        self.attemptedStart = true
                        self.startService()
                    }
                }
            }
        }
    }
    func accept(_ response: [String: Any], saved: Bool = false) {
        guard response["protocol"] as? Int == 1, let state = response["snapshot"] as? [String: Any],
              let config = state["config"] as? [String: Any], let revision = state["revision"] as? UInt64,
              let instance = state["instance"] as? String else {
            connected = false
            error = "Unsupported or incomplete service response."
            saving = false
            return
        }
        connected = true
        snapshot = state
        if !dirty || saved || self.revision == nil {
            replace(config: config, revision: revision, instance: instance)
        } else { conflicted = revision != self.revision || instance != self.instance }
        saving = false
        if !saved && !dirty { message = status }
        if saved { message = "Changes saved" }
    }
    private func replace(config: [String: Any], revision: UInt64, instance: String) {
        guard let profiles = config["profiles"] as? [String: [String: Any]], profiles["default"] != nil,
              let settings = config["settings"] as? [String: Any] else { error = "Invalid configuration from service."; return }
        var next = SettingsData()
        let selectedID = data.profiles.isEmpty ? "default" : data.profiles[profile].id
        for id in profiles.keys.sorted(by: { $0 == "default" ? $1 != "default" : $1 != "default" && $0 < $1 }) {
            let p = profiles[id]!
            let mappings = p["mappings"] as? [String: String] ?? [:]
            next.profiles.append(Profile(id: id, name: p["label"] as? String ?? id, application: (p["apps"] as? [String] ?? []).joined(separator: "\n"), actions: controlKeys.map { label(mappings[$0] ?? "none") }))
        }
        next.dpi = settings["dpi"] as? Double ?? 1000
        next.verticalInverted = settings["invert_vscroll"] as? Bool ?? false
        next.horizontalInverted = settings["invert_hscroll"] as? Bool ?? false
        next.smartShift = settings["smart_shift_enabled"] as? Bool ?? false
        next.threshold = settings["smart_shift_threshold"] as? Double ?? 25
        next.startAtLogin = settings["start_at_login"] as? Bool ?? false
        next.ignoreTrackpad = settings["ignore_trackpad"] as? Bool ?? true
        next.wheelMode = settings["smart_shift_mode"] as? String ?? "ratchet"
        next.presets = (settings["dpi_presets"] as? [Int] ?? [800, 1200, 1600, 2400]).map(String.init).joined(separator: ", ")
        next.gestureThreshold = settings["gesture_threshold"] as? Double ?? 50
        next.gestureDeadzone = settings["gesture_deadzone"] as? Double ?? 40
        next.gestureTimeout = settings["gesture_timeout_ms"] as? Double ?? 3000
        next.gestureCooldown = settings["gesture_cooldown_ms"] as? Double ?? 500
        data = next
        baseline = next
        theme = (settings["appearance_mode"] as? String ?? "system").capitalized
        baselineTheme = theme
        baselineConfig = config
        self.revision = revision
        self.instance = instance
        profile = next.profiles.firstIndex(where: { $0.id == selectedID }) ?? 0
        conflicted = false
    }
    func assign(_ value: String) {
        guard !data.profiles.isEmpty else { return }
        let id = identifier(value)
        do {
            _ = try NativeBridge.call(["local": "validate_action", "action": id])
            data.profiles[profile].actions[selected] = label(id)
        } catch { self.error = error.localizedDescription }
    }
    func addProfile(name: String, application: String) -> Bool {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name.count <= 100, data.profiles.count < 64, !data.profiles.contains(where: { $0.name.lowercased() == name.lowercased() }) else {
            error = "Use a unique name of 1–100 characters. Up to 64 profiles are supported."
            return false
        }
        data.profiles.append(Profile(id: "profile_" + UUID().uuidString.lowercased(), name: name, application: application, actions: data.profiles[0].actions))
        profile = data.profiles.count - 1
        return true
    }
    func deleteProfile() { if !data.profiles.isEmpty && data.profiles[profile].id != "default" { data.profiles.remove(at: profile); profile = 0 } }
    func revert() {
        guard let config = snapshot["config"] as? [String: Any], let revision = snapshot["revision"] as? UInt64, let instance = snapshot["instance"] as? String else { return }
        replace(config: config, revision: revision, instance: instance)
        message = "Loaded the latest service configuration."
    }
    func makeConfig() throws -> [String: Any] {
        var config = baselineConfig
        var profiles = config["profiles"] as? [String: [String: Any]] ?? [:]
        for id in Array(profiles.keys) where !data.profiles.contains(where: { $0.id == id }) { profiles.removeValue(forKey: id) }
        for profile in data.profiles {
            var stored = profiles[profile.id] ?? [:]
            stored["label"] = profile.name
            stored["apps"] = profile.application.components(separatedBy: .newlines).map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
            var mappings = stored["mappings"] as? [String: Any] ?? [:]
            guard !profile.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, profile.name.count <= 100 else { throw ServiceFailure(message: "Profile names must contain 1–100 characters.") }
            let old = baseline.profiles.first(where: { $0.id == profile.id })
            for (index, key) in controlKeys.enumerated() where old == nil || old!.actions[index] != profile.actions[index] {
                mappings[key] = identifier(profile.actions[index])
            }
            stored["mappings"] = mappings
            profiles[profile.id] = stored
        }
        config["profiles"] = profiles
        var settings = config["settings"] as? [String: Any] ?? [:]
        let fields: [(String, Any, Any)] = [
            ("dpi", data.dpi, baseline.dpi), ("invert_vscroll", data.verticalInverted, baseline.verticalInverted),
            ("invert_hscroll", data.horizontalInverted, baseline.horizontalInverted), ("smart_shift_enabled", data.smartShift, baseline.smartShift),
            ("smart_shift_threshold", data.threshold, baseline.threshold), ("start_at_login", data.startAtLogin, baseline.startAtLogin),
            ("ignore_trackpad", data.ignoreTrackpad, baseline.ignoreTrackpad), ("smart_shift_mode", data.wheelMode, baseline.wheelMode),
            ("gesture_threshold", data.gestureThreshold, baseline.gestureThreshold), ("gesture_deadzone", data.gestureDeadzone, baseline.gestureDeadzone),
            ("gesture_timeout_ms", data.gestureTimeout, baseline.gestureTimeout), ("gesture_cooldown_ms", data.gestureCooldown, baseline.gestureCooldown)
        ]
        for (key, value, previous) in fields where !NSDictionary(dictionary: ["v": value]).isEqual(to: ["v": previous]) { settings[key] = value }
        if data.presets != baseline.presets {
            let pieces = data.presets.split(separator: ",", omittingEmptySubsequences: false)
            let values = pieces.compactMap { Int($0.trimmingCharacters(in: .whitespaces)) }
            guard values.count == pieces.count, (1...16).contains(values.count), values.allSatisfy({ (200...8000).contains($0) }) else {
                throw ServiceFailure(message: "Enter 1–16 comma-separated DPI values between 200 and 8000.")
            }
            settings["dpi_presets"] = values
        }
        if theme != baselineTheme { settings["appearance_mode"] = theme.lowercased() }
        config["settings"] = settings
        return config
    }
    func save() {
        guard canSave, let revision, let instance else { return }
        do {
            let config = try makeConfig()
            saving = true
            request(["expected_instance": instance, "request": ["command": "apply", "expected_revision": revision, "config": config]]) { response in self.accept(response, saved: true) }
        } catch let failure { self.error = failure.localizedDescription; saving = false }
    }
    func openPrivacySettings(inputMonitoring: Bool = false) {
        let section = inputMonitoring ? "Privacy_ListenEvent" : "Privacy_Accessibility"
        guard let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?" + section) else { return }
        if !NSWorkspace.shared.open(url) { error = "Open System Settings → Privacy & Security manually." }
    }

    func startService() {
        guard !busy else { return }
        attemptedStart = true
        if let serviceProcess, serviceProcess.isRunning { message = "Starting Rust service…"; return }
        let executable = URL(fileURLWithPath: CommandLine.arguments[0]).deletingLastPathComponent().appendingPathComponent("superlight")
        guard FileManager.default.isExecutableFile(atPath: executable.path) else { error = "The bundled Rust service is missing. Reinstall the complete app."; return }
        let process = Process()
        process.executableURL = executable
        process.arguments = ["--background"]
        do { try process.run(); serviceProcess = process; message = "Starting Rust service…" }
        catch { self.error = error.localizedDescription }
    }
}
