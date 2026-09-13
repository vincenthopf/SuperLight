import SwiftUI
import AppKit

struct InspectorView: View {
    @ObservedObject var model: ServiceModel
    let images: [String: NSImage]
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var discard = false
    @State private var showInspector = true
    var mouseImage: NSImage? {
        let layout = model.device["layout_key"] as? String ?? ""
        if layout.contains("master") { return images["mouse"] }
        if layout.contains("anywhere") { return images["mouse_mx_anywhere_3s"] }
        if layout.contains("vertical") { return images["mx_vertical"] }
        return nil
    }
    var body: some View {
        NavigationSplitView {
            List(selection: $model.page) {
                Section("Mouse") {
                    ForEach(Page.allCases, id: \.self) { page in Label(page.rawValue, systemImage: page.icon).tag(page) }
                }
            }
            .listStyle(.sidebar)
            .navigationSplitViewColumnWidth(min: 180, ideal: 210, max: 280)
            .safeAreaInset(edge: .bottom) {
                VStack(alignment: .leading, spacing: 8) {
                    Label(model.status, systemImage: model.status == "Mouse ready" ? "checkmark.circle" : "info.circle").font(.caption)
                    Text(model.connectionSummary).font(.caption2).foregroundStyle(.secondary).lineLimit(2)
                }.frame(maxWidth: .infinity, alignment: .leading).padding()
            }
        } detail: {
            detail
                .navigationTitle(model.page == .buttons ? model.deviceName : model.page.rawValue)
                .toolbar {
                    ToolbarItemGroup(placement: .primaryAction) {
                        Button(model.connected ? "Refresh device settings" : "Start service", systemImage: "arrow.clockwise") {
                            if model.connected { model.command("refresh_hardware") } else { model.startService() }
                        }.disabled(model.busy)
                        Button(model.snapshot["paused"] as? Bool == true ? "Resume" : "Pause", systemImage: model.snapshot["paused"] as? Bool == true ? "play" : "pause") {
                            model.command("set_paused", value: model.snapshot["paused"] as? Bool != true)
                        }.disabled(!model.connected || model.busy)
                        if model.page == .buttons {
                            Button("Toggle Inspector", systemImage: "sidebar.right") { showInspector.toggle() }
                        }
                    }
                }
                .inspector(isPresented: Binding(get: { showInspector && model.page == .buttons && !model.data.profiles.isEmpty }, set: { showInspector = $0 })) {
                    Form {
                        Section { profilePicker }
                        Section("Selected control") {
                            if !model.data.profiles.isEmpty && model.supportedButton(model.selected) { ActionEditor(model: model) }
                            else { Text("This control is unavailable on the connected device.").foregroundStyle(.secondary) }
                        }
                        Section("Currently active") {
                            LabeledContent("Profile", value: model.snapshot["active_profile"] as? String ?? "default")
                            LabeledContent("Application", value: (model.snapshot["foreground"] as? [String: Any])?["name"] as? String ?? "—")
                        }
                    }.formStyle(.grouped).inspectorColumnWidth(min: 290, ideal: 320, max: 400).disabled(model.saving)
                }
                .safeAreaInset(edge: .bottom, spacing: 0) { saveBar }
        }
        .navigationSplitViewStyle(.balanced)
        .preferredColorScheme(model.scheme)
        .alert("Discard unsaved changes?", isPresented: $discard) {
            Button("Keep editing", role: .cancel) {}
            Button("Reload", role: .destructive) { model.revert() }
        } message: { Text("Load the latest configuration reported by the service.") }
        .alert("SuperLight", isPresented: Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })) { Button("OK") { model.error = nil } } message: { Text(model.error ?? "") }
    }
    @ViewBuilder var detail: some View {
        if model.data.profiles.isEmpty {
            ContentUnavailableView {
                Label("Connect to SuperLight", systemImage: "computermouse")
            } description: { Text(model.message) } actions: {
                Button("Start service") { model.startService() }.disabled(model.busy)
            }
        } else {
            VStack(spacing: 0) {
                if model.status != "Mouse ready" {
                    HStack {
                        Label(model.status, systemImage: "info.circle").font(.callout)
                        Spacer()
                        if model.status == "Input permissions required" {
                            Button("Review permissions") { model.page = .settings }
                        }
                    }.padding().background(.quaternary.opacity(0.5))
                    .transition(.opacity)
                }
                switch model.page {
                case .buttons:
                    VStack(spacing: 0) {
                        if let mouseImage {
                            MouseDiagram(model: model, mouseImage: mouseImage).frame(height: 260).padding(.horizontal, 12)
                        } else {
                            ContentUnavailableView(model.device.isEmpty ? "No mouse detected" : "Mouse controls", systemImage: "computermouse", description: Text("A diagram appears for recognized models. Your assignments are kept while disconnected.")).frame(height: 220)
                        }
                        Text(deviceDescription).font(.caption).foregroundStyle(.secondary).padding(.bottom, 12)
                        ButtonList(model: model)
                    }
                case .scroll: ScrollSettings(model: model)
                case .profiles: ProfileSettings(model: model)
                case .settings: GeneralSettings(model: model)
                }
            }.disabled(model.saving)
                .animation(.timingCurve(0.25, 1, 0.5, 1, duration: reduceMotion ? 0.1 : 0.18), value: model.status)
        }
    }
    var saveBar: some View {
        VStack(spacing: 8) {
            Divider()
            if model.conflicted { Text("The service configuration changed. Reload before saving.").font(.caption).foregroundStyle(.orange) }
            HStack(spacing: 10) {
                if model.saving { ProgressView().controlSize(.small) }
                Text(model.saving ? "Saving…" : model.dirty ? "Unsaved changes" : "All changes saved").font(.caption).foregroundStyle(.secondary).lineLimit(2)
                Spacer()
                Button(model.conflicted ? "Reload" : "Revert") { discard = true }.disabled((!model.dirty && !model.conflicted) || model.saving)
                Button("Save changes") { model.save() }.keyboardShortcut("s", modifiers: .command).disabled(!model.canSave).buttonStyle(.borderedProminent)
            }.padding(.horizontal).padding(.bottom, 12)
        }.background(.bar)
            .animation(.easeOut(duration: reduceMotion ? 0.1 : 0.16), value: model.saving)
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
        }
    }
}

struct GeneralSettings: View {
    @ObservedObject var model: ServiceModel
    var body: some View {
        Form {
            Section("Permissions & connection") {
                LabeledContent("Input Monitoring", value: model.permission["listen"] as? Bool == true ? "Allowed" : "Required")
                LabeledContent("Accessibility", value: model.permission["inject"] as? Bool == true ? "Allowed" : "Required")
                Text(model.permission["description"] as? String ?? "").font(.caption).foregroundStyle(.secondary)
                HStack {
                    Button("Request permissions") { model.command("request_permissions") }.disabled(model.busy || !model.connected)
                    Button("Reconnect mouse") { model.command("reconnect") }.disabled(model.busy || !model.connected)
                }
            }
            Section("Startup & appearance") {
                Toggle("Start at login", isOn: $model.data.startAtLogin)
                Picker("Appearance", selection: $model.theme) { ForEach(["Light", "Dark", "System"], id: \.self) { Text($0) } }
                Text("Closing settings leaves the Rust service running. Save to apply startup and appearance changes.").font(.caption).foregroundStyle(.secondary)
            }
            Section("Gestures") {
                Slider(value: $model.data.gestureThreshold, in: 10...300, step: 1) { Text("Distance threshold") }
                Slider(value: $model.data.gestureDeadzone, in: 0...200, step: 1) { Text("Dead zone") }
                Slider(value: $model.data.gestureTimeout, in: 100...10000, step: 100) { Text("Timeout (ms)") }
                Slider(value: $model.data.gestureCooldown, in: 0...2000, step: 50) { Text("Cooldown (ms)") }
            }.disabled(!model.supports("supports_gesture"))
            Section("Diagnostics") {
                LabeledContent("Dropped events", value: String(model.snapshot["dropped_events"] as? Int ?? 0))
                LabeledContent("Hardware update", value: model.snapshot["hardware_pending"] as? Bool == true ? "Pending" : "Idle")
                if let notice = model.snapshot["notice"] as? String { Text(notice).foregroundStyle(.orange) }
                ForEach(Array((model.snapshot["errors"] as? [String] ?? []).enumerated()), id: \.offset) { _, text in Text(text).font(.caption).textSelection(.enabled) }
            }
        }.formStyle(.grouped)
    }
}
