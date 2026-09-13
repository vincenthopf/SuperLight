import SwiftUI

struct ScrollSettings: View {
    @ObservedObject var model: ServiceModel
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                VStack(alignment: .leading, spacing: 16) {
                    Text("Pointer speed").font(.headline)
                    HStack { Text("Sensitivity"); Spacer(); Text("\(Int(model.data.dpi)) DPI").monospacedDigit().foregroundStyle(.secondary) }
                    Slider(value: $model.data.dpi, in: model.dpiBounds, step: 50) { Text("Pointer DPI") }
                    HStack { Text("Precise"); Spacer(); Text("Fast") }.font(.caption).foregroundStyle(.secondary)
                    HStack { ForEach([800, 1000, 1600, 2400], id: \.self) { value in Button("\(value)") { model.data.dpi = Double(value) } } }
                }.padding(22).modifier(Surface()).disabled(!model.supports("supports_dpi"))
                VStack(alignment: .leading, spacing: 18) {
                    Text("Scroll wheel").font(.headline)
                    Toggle("SmartShift", isOn: $model.data.smartShift).toggleStyle(.switch).disabled(!model.supports("supports_smart_shift"))
                    Text("Automatically switch between precise ratchet steps and free spin.").font(.caption).foregroundStyle(.secondary)
                    Slider(value: $model.data.threshold, in: 1...50, step: 1) { Text("SmartShift threshold") }.disabled(!model.data.smartShift || !model.supports("supports_smart_shift"))
                    Picker("Wheel mode", selection: $model.data.wheelMode) { Text("Ratchet").tag("ratchet"); Text("Free spin").tag("freespin") }.disabled(!model.supports("supports_smart_shift"))
                    TextField("DPI presets", text: $model.data.presets).textFieldStyle(.roundedBorder).disabled(!model.supports("supports_dpi"))
                    Divider()
                    Toggle("Invert vertical scrolling", isOn: $model.data.verticalInverted)
                    Toggle("Invert horizontal scrolling", isOn: $model.data.horizontalInverted)
                    Toggle("Leave trackpad scrolling unchanged", isOn: $model.data.ignoreTrackpad)
                }.padding(22).modifier(Surface())
                Text("Save to send these values to the connected mouse. Unsupported hardware controls are disabled.").font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}

struct ProfileSettings: View {
    @ObservedObject var model: ServiceModel
    @State private var adding = false
    @State private var deleting = false
    @State private var name = ""
    @State private var application = ""
    @FocusState private var nameFocused: Bool
    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("Keep a different set of actions for each app. New profiles start with your default assignments.").foregroundStyle(.secondary)
            ForEach(Array(model.data.profiles.enumerated()), id: \.element.id) { index, profile in
                Button { model.profile = index } label: {
                    HStack {
                        Image(systemName: index == 0 ? "square.grid.2x2" : "app").font(.title3).frame(width: 36)
                        VStack(alignment: .leading, spacing: 4) { Text(profile.name).fontWeight(.medium); Text(profile.application.isEmpty ? "Default for every app" : profile.application).font(.caption).foregroundStyle(.secondary) }
                        Spacer()
                        if model.profile == index { Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.accentColor) }
                    }.padding(16).modifier(Surface()).contentShape(Rectangle())
                }.buttonStyle(.plain)
            }
            HStack {
                Button("Add profile", systemImage: "plus") { adding = true }.disabled(model.data.profiles.count >= 64)
                Spacer()
                Button("Delete selected profile", role: .destructive) { deleting = true }.disabled(model.profile == 0)
            }
            Spacer()
        }
        .sheet(isPresented: $adding) {
            VStack(alignment: .leading, spacing: 16) {
                Text("New profile").font(.title2.weight(.semibold))
                TextField("Profile name", text: $name).textFieldStyle(.roundedBorder).focused($nameFocused)
                TextField("Application identifier, e.g. com.apple.Safari", text: $application).textFieldStyle(.roundedBorder)
                Text("Use an application bundle identifier or executable name.").font(.caption).foregroundStyle(.secondary)
                HStack { Button("Cancel") { adding = false }.keyboardShortcut(.cancelAction); Spacer(); Button("Create profile") {
                    if model.addProfile(name: name, application: application) { name = ""; application = ""; adding = false }
                }.keyboardShortcut(.defaultAction).disabled(name.trimmingCharacters(in: .whitespaces).isEmpty) }
            }.padding(24).frame(width: 410).onAppear { nameFocused = true }
        }
        .alert("Delete this profile?", isPresented: $deleting) {
            Button("Cancel", role: .cancel) {}
            Button("Delete", role: .destructive) { model.deleteProfile() }
        } message: { Text("Its button assignments will be removed. You can revert before saving.") }
    }
}

