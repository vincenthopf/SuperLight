import SwiftUI
import AppKit

struct ScrollSettings: View {
    @Bindable var model: ServiceModel
    var body: some View {
        Form {
            Section("Pointer speed") {
                SettingSlider(title: "Sensitivity", value: $model.data.dpi, range: model.dpiBounds, step: 50, unit: "DPI")
                HStack { ForEach([800, 1000, 1600, 2400], id: \.self) { value in Button("\(value)") { model.data.dpi = Double(value) }.disabled(!model.dpiBounds.contains(Double(value))) } }
                TextField("DPI presets", text: $model.data.presets)
            }.disabled(!model.supports("supports_dpi"))
            Section("Scroll wheel") {
                Toggle("SmartShift", isOn: $model.data.smartShift).disabled(!model.supports("supports_smart_shift"))
                Text("Automatically switch between precise ratchet steps and free spin.").font(.caption).foregroundStyle(.secondary)
                SettingSlider(title: "SmartShift threshold", value: $model.data.threshold, range: 1...50, step: 1).disabled(!model.data.smartShift || !model.supports("supports_smart_shift"))
                Picker("Wheel mode", selection: $model.data.wheelMode) { Text("Ratchet").tag("ratchet"); Text("Free spin").tag("freespin") }.disabled(!model.supports("supports_smart_shift"))
                Toggle("Invert vertical scrolling", isOn: $model.data.verticalInverted)
                Toggle("Invert horizontal scrolling", isOn: $model.data.horizontalInverted)
                Toggle("Leave trackpad scrolling unchanged", isOn: $model.data.ignoreTrackpad)
            }
            Section {
                Text("Save to send changes to the service. Unsupported hardware controls are disabled.").font(.caption).foregroundStyle(.secondary)
            }
        }.formStyle(.grouped)
    }
}

struct ProfileSettings: View {
    @Bindable var model: ServiceModel
    @State private var adding = false
    @State private var deleting = false
    @State private var name = ""
    @State private var application = ""
    @State private var validation = ""
    @FocusState private var nameFocused: Bool
    var body: some View {
        Form {
            Section("Profiles") {
                ProfilePicker(model: model)
                if !model.data.profiles.isEmpty {
                    TextField("Profile name", text: $model.data.profiles[model.profile].name).disabled(model.data.profiles[model.profile].id == "default")
                    Text("Application identifiers, one per line").font(.callout)
                    TextEditor(text: $model.data.profiles[model.profile].application).frame(height: 100).accessibilityLabel("Application identifiers").disabled(model.data.profiles[model.profile].id == "default")
                    Button("Use current application") {
                        if let id = (model.snapshot["foreground"] as? [String: Any])?["id"] as? String, !id.isEmpty {
                            model.data.profiles[model.profile].application = id
                        }
                    }.disabled(model.data.profiles[model.profile].id == "default" || ((model.snapshot["foreground"] as? [String: Any])?["id"] as? String ?? "").isEmpty)
                }
                HStack {
                    Button("Add profile", systemImage: "plus") { adding = true }.disabled(model.data.profiles.count >= 64)
                    Spacer()
                    Button("Delete profile", role: .destructive) { deleting = true }.disabled(model.profile == 0)
                }
                Text("New profiles start with your default assignments. App matching uses bundle identifiers or executable names.").font(.caption).foregroundStyle(.secondary)
            }
        }.formStyle(.grouped)
        .sheet(isPresented: $adding) {
            VStack(alignment: .leading, spacing: 18) {
                Text("New profile").font(.title2.weight(.semibold))
                Form {
                    TextField("Name", text: $name).focused($nameFocused)
                    TextField("Application", text: $application).help("For example com.apple.Safari")
                }
                if !validation.isEmpty { Text(validation).foregroundStyle(.red).font(.caption) }
                HStack {
                    Button("Cancel") { adding = false }.keyboardShortcut(.cancelAction)
                    Spacer()
                    Button("Create profile") {
                        if model.addProfile(name: name, application: application) { name = ""; application = ""; validation = ""; adding = false }
                        else { validation = model.error ?? "Invalid profile"; model.error = nil }
                    }.keyboardShortcut(.defaultAction).disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }.padding(24).frame(width: 420).onAppear { nameFocused = true }
        }
        .alert("Delete this profile?", isPresented: $deleting) {
            Button("Cancel", role: .cancel) {}
            Button("Delete", role: .destructive) { model.deleteProfile() }
        } message: { Text("Its button assignments will be removed when you save. You can revert before saving.") }
    }
}

struct AboutPage: View {
    var body: some View {
        VStack(spacing: 16) {
            Image(nsImage: NSApplication.shared.applicationIconImage).resizable().frame(width: 96, height: 96).accessibilityHidden(true)
            Text("SuperLight").font(.largeTitle.weight(.semibold))
            Text("Native Logitech mouse controls").foregroundStyle(.secondary)
            if let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String {
                Text("Version \(version)").font(.caption).foregroundStyle(.tertiary)
            }
            Link("View on GitHub", destination: URL(string: "https://github.com/vincenthopf/SuperLight")!)
            Text("Configure buttons, scrolling, gestures, and profiles from a native macOS interface.").multilineTextAlignment(.center).frame(maxWidth: 360)
        }.frame(maxWidth: .infinity, maxHeight: .infinity).padding(40)
    }
}
