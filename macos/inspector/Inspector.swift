import SwiftUI
import AppKit

struct InspectorView: View {
    @Bindable var model: ServiceModel
    let images: [String: NSImage]
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
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
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 10) {
                    Image(nsImage: NSApplication.shared.applicationIconImage)
                        .resizable().frame(width: 36, height: 36)
                        .accessibilityHidden(true)
                    Text("SuperLight").font(.headline).lineLimit(1)
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 16)
                .padding(.top, 14)
                .padding(.bottom, 10)
                List(selection: $model.page) {
                    Section("Mouse") {
                        ForEach(Page.allCases, id: \.self) { page in
                            Label {
                                Text(page.rawValue).lineLimit(1)
                            } icon: {
                                if model.page == page {
                                    Image(systemName: page.icon)
                                } else {
                                    Image(systemName: page.icon).foregroundStyle(Color.accentColor)
                                }
                            }.tag(page).listRowSeparator(.hidden)
                        }
                    }
                }
                .listStyle(.sidebar)
                .scrollContentBackground(.hidden)
                Spacer(minLength: 0)
            }
            .frame(minWidth: 230, maxHeight: .infinity, alignment: .top)
            .background(.regularMaterial)
            .navigationSplitViewColumnWidth(min: 210, ideal: 240, max: 300)
            .toolbar(removing: .sidebarToggle)
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
                    }
                }
                .inspector(isPresented: .constant(model.page == .buttons && !model.data.profiles.isEmpty)) {
                    controlEditor.inspectorColumnWidth(min: 290, ideal: 320, max: 400)
                }
                .safeAreaInset(edge: .bottom, spacing: 0) { saveBar }
        }
        .navigationSplitViewStyle(.balanced)
        .buttonStyle(.bordered)
        .preferredColorScheme(model.scheme)
        .alert("Discard unsaved changes?", isPresented: $discard) {
            Button("Keep editing", role: .cancel) {}
            Button("Reload", role: .destructive) { model.revert() }
        } message: { Text("Load the latest configuration reported by the service.") }
        .alert("SuperLight", isPresented: Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })) { Button("OK") { model.error = nil } } message: { Text(model.error ?? "") }
    }
    var controlEditor: some View {
        GeometryReader { geometry in
            VStack(spacing: 12) {
                Form {
                    Section { profilePicker }
                    Section("Selected control") {
                        VStack(alignment: .leading, spacing: 18) {
                            if !(model.device["layout_key"] as? String ?? "").contains("master") || model.supportedButton(11) {
                                Picker("Control", selection: $model.selected) {
                                    ForEach(controlNames.indices, id: \.self) { index in
                                        if model.supportedButton(index) {
                                            Text(controlNames[index]).tag(index)
                                        }
                                    }
                                }
                            }
                            if model.supportedButton(model.selected) {
                                ActionEditor(model: model)
                            } else {
                                Text("This control is unavailable on the connected device.").foregroundStyle(.secondary)
                            }
                        }
                    }
                }
                .formStyle(.grouped)
                .frame(maxHeight: .infinity)
                .disabled(model.saving)
                statusCard
                    .frame(height: max(210, geometry.size.height / 3))
                    .padding(.horizontal, 12)
                    .padding(.bottom, 12)
            }
        }
    }

    @ViewBuilder private var statusCard: some View {
        if #available(macOS 26.0, *) {
            statusContent.glassEffect(.regular, in: RoundedRectangle(cornerRadius: 18))
        } else {
            statusContent.background(.regularMaterial, in: RoundedRectangle(cornerRadius: 18))
        }
    }

    private var statusContent: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                Label(model.status, systemImage: model.status == "Mouse ready" ? "checkmark.circle.fill" : "info.circle")
                    .font(.callout.weight(.medium))
                    .foregroundStyle(model.status == "Mouse ready" ? .green : .primary)
                StatusRow("Connection", value: model.device["transport"] as? String ?? "Not connected")
                StatusRow("Battery", value: (model.device["battery"] as? Int).map { "\($0)%" } ?? "Unavailable")
                StatusRow("Sensitivity", value: (model.device["dpi"] as? Int).map { "\($0) DPI" } ?? "Unavailable")
                StatusRow("Active profile", value: model.snapshot["active_profile"] as? String ?? "—")
                StatusRow("Application", value: (model.snapshot["foreground"] as? [String: Any])?["name"] as? String ?? "—")
            }
            .font(.caption)
            .monospacedDigit()
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(16)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private struct StatusRow: View {
        let title: String
        let value: String

        init(_ title: String, value: String) {
            self.title = title
            self.value = value
        }

        var body: some View {
            HStack(alignment: .firstTextBaseline) {
                Text(title).foregroundStyle(.secondary)
                Spacer(minLength: 12)
                Text(value).lineLimit(1).help(value)
            }
        }
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
                            Button("Open System Settings") { model.openPrivacySettings() }
                        }
                    }.padding().background(.quaternary.opacity(0.5))
                    .transition(.opacity)
                }
                switch model.page {
                case .buttons:
                    VStack(spacing: 0) {
                        if let mouseImage {
                            MouseDiagram(model: model, mouseImage: mouseImage).frame(minHeight: 400, idealHeight: 440, maxHeight: 500).padding(.horizontal, 12)
                        } else {
                            ContentUnavailableView(model.device.isEmpty ? "No mouse detected" : "Mouse controls", systemImage: "computermouse", description: Text("A diagram appears for recognized models. Your assignments are kept while disconnected.")).frame(height: 220)
                        }
                    }
                case .scroll: ScrollSettings(model: model)
                case .profiles: ProfileSettings(model: model)
                case .settings: GeneralSettings(model: model)
                case .about: AboutPage()
                }
            }.disabled(model.saving)
                .animation(.timingCurve(0.25, 1, 0.5, 1, duration: reduceMotion ? 0.1 : 0.18), value: model.status)
        }
    }
    var saveBar: some View {
        Group {
            if model.dirty || model.conflicted || model.saving {
        VStack(spacing: 8) {
            if model.conflicted { Text("The service configuration changed. Reload before saving.").font(.caption).foregroundStyle(.orange) }
            HStack(spacing: 10) {
                if model.saving { ProgressView().controlSize(.small) }
                Text(model.saving ? "Saving…" : model.dirty ? "Unsaved changes" : "All changes saved").font(.caption).foregroundStyle(.secondary).lineLimit(2)
                Spacer()
                Button(model.conflicted ? "Reload" : "Revert") { discard = true }.disabled((!model.dirty && !model.conflicted) || model.saving)
                Button("Save changes") { model.save() }.keyboardShortcut("s", modifiers: .command).disabled(!model.canSave).buttonStyle(.borderedProminent)
            }.padding(.horizontal).padding(.bottom, 12)
        }.background(.bar)
            }
        }
            .animation(.easeOut(duration: reduceMotion ? 0.1 : 0.16), value: model.saving)
    }
    var profilePicker: some View {
        ProfilePicker(model: model)
    }
}

struct GeneralSettings: View {
    @Bindable var model: ServiceModel
    var body: some View {
        Form {
            Section("Permissions & connection") {
                LabeledContent("Input Monitoring", value: model.permission["listen"] as? Bool == true ? "Allowed" : "Required")
                LabeledContent("Accessibility", value: model.permission["inject"] as? Bool == true ? "Allowed" : "Required")
                Text(model.permission["description"] as? String ?? "").font(.caption).foregroundStyle(.secondary)
                HStack {
                    Button("Accessibility settings") { model.openPrivacySettings() }
                    Button("Input Monitoring settings") { model.openPrivacySettings(inputMonitoring: true) }
                }
                Text("Enable the installed SuperLight.app in both lists. No permission is changed automatically.").font(.caption).foregroundStyle(.secondary)
                HStack {
                    Button("Request permissions") { model.command("request_permissions") }.disabled(model.busy || !model.connected)
                    Button("Reconnect mouse") { model.command("reconnect") }.disabled(model.busy || !model.connected)
                }
            }
            Section("Startup & appearance") {
                Toggle("Start at login", isOn: $model.data.startAtLogin)
                Picker("Appearance", selection: $model.theme) { ForEach(["Light", "Dark", "System"], id: \.self) { Text($0) } }
                Text("Closing settings keeps your mouse assignments running. Save to apply startup and appearance changes.").font(.caption).foregroundStyle(.secondary)
            }
            Section("Gestures") {
                SettingSlider(title: "Distance threshold", value: $model.data.gestureThreshold, range: 10...300, step: 1, unit: "")
                SettingSlider(title: "Dead zone", value: $model.data.gestureDeadzone, range: 0...200, step: 1, unit: "")
                SettingSlider(title: "Timeout", value: $model.data.gestureTimeout, range: 100...10000, step: 100, unit: "ms")
                SettingSlider(title: "Cooldown", value: $model.data.gestureCooldown, range: 0...2000, step: 50, unit: "ms")
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
