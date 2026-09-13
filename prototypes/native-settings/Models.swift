import Foundation
import SwiftUI

let controlNames = ["Middle click", "Thumb gesture", "Back", "Forward", "Scroll left", "Scroll right", "Wheel mode", "Gesture left", "Gesture right", "Gesture up", "Gesture down"]
let actionGroups: [(String, [String])] = [
    ("Mouse", ["Default behavior", "Middle click", "Switch scroll mode"]),
    ("Desktop", ["Mission Control", "App Exposé", "Show desktop", "Switch applications"]),
    ("Navigation", ["Browser back", "Browser forward", "Previous tab", "Next tab"]),
    ("Media", ["Play / pause", "Mute", "Volume up", "Volume down"])
]

enum Direction: String, CaseIterable {
    case workbench = "Workbench", inspector = "Inspector", command = "Command Center"
    var number: String { switch self { case .workbench: "01"; case .inspector: "02"; case .command: "03" } }
    var subtitle: String { switch self {
    case .workbench: "Select on the mouse. Configure below."
    case .inspector: "A dedicated editor for every control."
    case .command: "Every assignment, in one place."
    } }
}

enum Page: String, CaseIterable {
    case buttons = "Buttons", scroll = "Point & scroll", profiles = "Profiles", settings = "Settings"
    var icon: String { switch self {
    case .buttons: "computermouse"
    case .scroll: "arrow.up.arrow.down"
    case .profiles: "square.stack"
    case .settings: "slider.horizontal.3"
    } }
}

struct Profile: Codable, Equatable, Identifiable {
    var id = UUID()
    var name: String
    var application: String
    var actions = ["Middle click", "Mission Control", "Browser back", "Browser forward", "Previous tab", "Next tab", "Switch scroll mode", "Default behavior", "Default behavior", "Default behavior", "Default behavior"]
}

struct PreviewData: Codable, Equatable {
    var profiles = [Profile(name: "All applications", application: ""), Profile(name: "Browser", application: "com.apple.Safari")]
    var dpi = 1000.0
    var verticalInverted = false
    var horizontalInverted = false
    var smartShift = true
    var threshold = 25.0
    var startAtLogin = false
    var ignoreTrackpad = true
}

@MainActor final class PreviewModel: ObservableObject {
    let direction: Direction
    let saveURL: URL
    @Published var data: PreviewData
    @Published var baseline: PreviewData
    @Published var page = Page.buttons
    @Published var selected = 0
    @Published var profile = 0
    @Published var theme = "Dark"
    @Published var message = "No changes to your physical mouse."
    @Published var error: String?
    @Published var deviceState = "Preview device"

    init(direction: Direction, directory: URL) {
        self.direction = direction
        saveURL = directory.appendingPathComponent("\(direction.number)-preview.json")
        let initial = PreviewData()
        data = initial
        baseline = initial
        if FileManager.default.fileExists(atPath: saveURL.path) {
            do {
                let decoded = try JSONDecoder().decode(PreviewData.self, from: Data(contentsOf: saveURL))
                guard !decoded.profiles.isEmpty, decoded.profiles.count <= 16,
                      decoded.profiles.allSatisfy({ $0.actions.count == controlNames.count }),
                      (200...8000).contains(decoded.dpi), (1...50).contains(decoded.threshold) else {
                    throw CocoaError(.coderReadCorrupt)
                }
                data = decoded
                baseline = decoded
            } catch { self.error = "Could not load the saved preview: \(error.localizedDescription)" }
        }
    }

    var dirty: Bool { data != baseline }
    var action: String { data.profiles[profile].actions[selected] }
    var scheme: ColorScheme? { theme == "System" ? nil : theme == "Dark" ? .dark : .light }

    func assign(_ value: String) {
        data.profiles[profile].actions[selected] = value
        message = "\(controlNames[selected]) updated in \(data.profiles[profile].name)."
    }

    func addProfile(name: String, application: String) -> Bool {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name.count <= 60, data.profiles.count < 16 else {
            error = "Use a name of 1–60 characters. Up to 16 preview profiles are supported."
            return false
        }
        guard !data.profiles.contains(where: { $0.name.lowercased() == name.lowercased() }) else {
            error = "A profile with this name already exists."
            return false
        }
        data.profiles.append(Profile(name: name, application: application.trimmingCharacters(in: .whitespacesAndNewlines), actions: data.profiles[0].actions))
        profile = data.profiles.count - 1
        return true
    }

    func deleteProfile() {
        guard profile > 0 else { return }
        data.profiles.remove(at: profile)
        profile = 0
    }

    func revert() {
        data = baseline
        profile = min(profile, data.profiles.count - 1)
        message = "Restored the last saved preview."
    }

    func save() {
        do {
            try FileManager.default.createDirectory(at: saveURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            try JSONEncoder().encode(data).write(to: saveURL, options: .atomic)
            baseline = data
            message = "Preview saved locally. Mouse settings unchanged."
        } catch { self.error = "Could not save the preview: \(error.localizedDescription)" }
    }
}
