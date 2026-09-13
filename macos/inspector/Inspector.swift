import SwiftUI
import AppKit

struct InspectorView: View {
    @ObservedObject var model: ServiceModel
    let images: [String: NSImage]
    @Environment(\.colorScheme) var scheme
    @State private var discard = false
    var mouseImage: NSImage? {
        let layout = model.device["layout_key"] as? String ?? ""
        if layout.contains("master") { return images["mouse"] }
        if layout.contains("anywhere") { return images["mouse_mx_anywhere_3s"] }
        if layout.contains("vertical") { return images["mx_vertical"] }
        return nil
    }
    var body: some View {
        NavigationSplitView {
            sidebar
        } detail: {
            VStack(spacing: 0) {
            HStack {
                Label(model.status, systemImage: model.status == "Mouse ready" ? "checkmark.circle" : "info.circle").font(.caption)
                Spacer()
                if model.busy { ProgressView().controlSize(.small) }
                Picker("Appearance", selection: $model.theme) { ForEach(["Light", "Dark", "System"], id: \.self) { Text($0) } }.labelsHidden().frame(width: 105).accessibilityLabel("Appearance").disabled(model.saving || model.data.profiles.isEmpty)
                Button(model.connected ? "Refresh" : "Start service") { if model.connected { model.command("refresh_hardware") } else { model.startService() } }.disabled(model.busy)
            }.padding(.horizontal, 22).frame(height: 46)
            Divider()
            HStack(spacing: 0) {
                if model.data.profiles.isEmpty {
                    ContentUnavailableView("Connect to SuperLight", systemImage: "computermouse", description: Text(model.message))
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if model.page == .buttons {
                    ScrollView {
                        VStack(alignment: .leading, spacing: 16) {
                            Text(model.deviceName).font(.system(size: 25, weight: .semibold))
                            Text(deviceDescription).foregroundStyle(.secondary).font(.caption)
                            if let mouseImage { MouseDiagram(model: model, mouseImage: mouseImage).frame(height: 265) }
                            else { ContentUnavailableView(model.device.isEmpty ? "No mouse detected" : "Mouse controls", systemImage: "computermouse", description: Text("Use the control list below. A diagram is shown for recognized models.")).frame(height: 160) }
                            Text("CONTROLS").font(.system(size: 10, weight: .semibold)).tracking(1).foregroundStyle(.secondary)
                            ButtonList(model: model)
                        }.padding(24)
                    }.frame(maxWidth: .infinity)
                    Divider()
                    ScrollView {
                        VStack(alignment: .leading, spacing: 24) {
                            Text("INSPECTOR").font(.system(size: 10, weight: .semibold)).tracking(1).foregroundStyle(.secondary)
                            profilePicker
                            Divider()
                            if model.supportedButton(model.selected) { ActionEditor(model: model) }
                            else { Text("This control is unavailable on the connected device.").foregroundStyle(.secondary) }
                            Divider()
                            Text("Active profile: \(model.snapshot["active_profile"] as? String ?? "default")").font(.caption).foregroundStyle(.secondary)
                            Text("\(model.snapshot["foreground"] as? [String: Any] != nil ? (model.snapshot["foreground"] as? [String: Any])?["name"] as? String ?? "" : "")").font(.caption).foregroundStyle(.secondary)
                        }.padding(24)
                    }.frame(width: 300).background(scheme == .dark ? Color(white: 0.105) : .white)
                } else {
                    VStack(alignment: .leading, spacing: 22) {
                        HStack { Text(model.page.rawValue).font(.system(size: 25, weight: .semibold)); Spacer(); profilePicker }
                        switch model.page {
                        case .scroll: ScrollSettings(model: model)
                        case .profiles: ProfileSettings(model: model)
                        case .settings: GeneralSettings(model: model)
                        case .buttons: EmptyView()
                        }
                    }.padding(28).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                }
            }.disabled(model.saving)
            Divider()
            if model.conflicted { Text("The service configuration changed. Reload before saving to avoid overwriting it.").font(.caption).foregroundStyle(.orange).padding(10) }
            HStack(spacing: 10) {
                Text(model.saving ? "Saving…" : model.dirty ? "Unsaved changes" : model.message).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                Spacer()
                Button(model.conflicted ? "Reload" : "Revert") { discard = true }.disabled((!model.dirty && !model.conflicted) || model.saving)
                Button("Save changes") { model.save() }.keyboardShortcut("s", modifiers: .command).disabled(!model.canSave).buttonStyle(.borderedProminent)
            }.padding(.horizontal, 22).frame(height: 56)
        }
        }
        .navigationSplitViewStyle(.balanced)
        .navigationTitle("SuperLight")
        .tint(Color(red: 0.33, green: 0.48, blue: 0.88)).preferredColorScheme(model.scheme)
        .alert("Discard unsaved changes?", isPresented: $discard) {
            Button("Keep editing", role: .cancel) {}
            Button("Reload", role: .destructive) { model.revert() }
        } message: { Text("Load the latest configuration reported by the service.") }
        .alert("SuperLight", isPresented: Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })) { Button("OK") { model.error = nil } } message: { Text(model.error ?? "") }
    }
    var deviceDescription: String {
        var parts = [model.device["transport"] as? String ?? model.status]
        if let battery = model.device["battery"] as? Int { parts.append("Battery \(battery)%") }
        if let dpi = model.device["dpi"] as? Int { parts.append("\(dpi) DPI") }
        return parts.joined(separator: " · ")
    }
    var profilePicker: some View {
        Picker("Editing profile", selection: $model.profile) {
            ForEach(Array(model.data.profiles.enumerated()), id: \.element.id) { index, profile in Text(profile.name).tag(index) }
        }.frame(maxWidth: 260)
    }
    var sidebar: some View {
        List(selection: $model.page) {
            Section("Workspace") {
                ForEach(Page.allCases, id: \.self) { page in
                    Label(page.rawValue, systemImage: page.icon).tag(page)
                }
            }
            Section("Service") {
                Label(model.status, systemImage: model.status == "Mouse ready" ? "checkmark.circle" : "info.circle")
                    .font(.caption).foregroundStyle(.secondary)
                Button(model.snapshot["paused"] as? Bool == true ? "Resume remapping" : "Pause remapping") {
                    model.command("set_paused", value: model.snapshot["paused"] as? Bool != true)
                }.disabled(!model.connected || model.busy)
            }
        }
        .listStyle(.sidebar)
        .navigationSplitViewColumnWidth(min: 190, ideal: 220, max: 280)
        .toolbar {
            ToolbarItem(placement: .automatic) {
                Label("SuperLight", systemImage: "computermouse.fill")
            }
        }
    }

}

struct GeneralSettings: View {
    @ObservedObject var model: ServiceModel
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                VStack(alignment: .leading, spacing: 18) {
                    Text("Permissions & connection").font(.headline)
                    Label("Input Monitoring: \(model.permission["listen"] as? Bool == true ? "Allowed" : "Required")", systemImage: "cursorarrow")
                    Label("Accessibility: \(model.permission["inject"] as? Bool == true ? "Allowed" : "Required")", systemImage: "accessibility")
                    Text(model.permission["description"] as? String ?? "").font(.caption).foregroundStyle(.secondary)
                    HStack {
                        Button("Request permissions") { model.command("request_permissions") }.disabled(model.busy || !model.connected)
                        Button("Reconnect mouse") { model.command("reconnect") }.disabled(model.busy || !model.connected)
                    }
                }.padding(22).modifier(Surface())
                VStack(alignment: .leading, spacing: 18) {
                    Text("Startup & appearance").font(.headline)
                    Toggle("Start at login", isOn: $model.data.startAtLogin)
                    Picker("Theme", selection: $model.theme) { ForEach(["Light", "Dark", "System"], id: \.self) { Text($0) } }.pickerStyle(.segmented)
                    Text("Closing settings leaves the Rust service running. Save to apply startup and appearance changes.").font(.caption).foregroundStyle(.secondary)
                }.padding(22).modifier(Surface())
                VStack(alignment: .leading, spacing: 12) {
                    Text("Gestures").font(.headline)
                    Slider(value: $model.data.gestureThreshold, in: 10...300, step: 1) { Text("Distance threshold") }
                    Slider(value: $model.data.gestureDeadzone, in: 0...200, step: 1) { Text("Dead zone") }
                    Slider(value: $model.data.gestureTimeout, in: 100...10000, step: 100) { Text("Timeout (ms)") }
                    Slider(value: $model.data.gestureCooldown, in: 0...2000, step: 50) { Text("Cooldown (ms)") }
                }.padding(22).modifier(Surface()).disabled(!model.supports("supports_gesture"))
                VStack(alignment: .leading, spacing: 12) {
                    Text("Diagnostics").font(.headline)
                    Text("Dropped events: \(model.snapshot["dropped_events"] as? Int ?? 0)").monospacedDigit()
                    Text("Hardware update: \(model.snapshot["hardware_pending"] as? Bool == true ? "Pending" : "Idle")")
                    if let notice = model.snapshot["notice"] as? String { Text(notice).foregroundStyle(.orange) }
                    ForEach(Array((model.snapshot["errors"] as? [String] ?? []).enumerated()), id: \.offset) { _, text in Text(text).font(.caption).textSelection(.enabled) }
                }.padding(22).modifier(Surface())
            }
        }
    }
}
