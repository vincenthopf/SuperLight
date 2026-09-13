import SwiftUI
import AppKit

struct DesignView: View {
    @ObservedObject var model: PreviewModel
    let mouseImage: NSImage
    @Environment(\.colorScheme) var scheme
    @State private var filter = ""
    @State private var discard = false
    var canvasColor: Color { scheme == .dark ? Color(red: 0.075, green: 0.08, blue: 0.09) : Color(red: 0.965, green: 0.966, blue: 0.974) }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Text(model.direction.number).font(.system(size: 10, weight: .semibold, design: .monospaced)).foregroundStyle(.secondary)
                Text(model.direction.rawValue).font(.system(size: 12, weight: .medium))
                Spacer()
                Text("DESIGN PREVIEW").font(.system(size: 9, weight: .semibold)).tracking(1).foregroundStyle(.secondary)
                Picker("Appearance", selection: $model.theme) {
                    ForEach(["Light", "Dark", "System"], id: \.self) { Text($0) }
                }.labelsHidden().frame(width: 100).accessibilityLabel("Appearance")
            }.padding(.horizontal, 22).frame(height: 44)
            Divider()
            if model.direction == .workbench { workbench }
            else if model.direction == .inspector { inspector }
            else { commandCenter }
            Divider()
            HStack(spacing: 8) {
                Image(systemName: model.dirty ? "circle.fill" : "checkmark.circle").font(.system(size: 9)).foregroundStyle(model.dirty ? Color.orange : Color.secondary)
                Text(model.dirty ? "Unsaved preview changes" : model.message).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                Spacer()
                Button("Revert") { discard = true }.disabled(!model.dirty)
                Button("Save preview") { model.save() }.keyboardShortcut("s", modifiers: .command).disabled(!model.dirty).buttonStyle(.borderedProminent)
            }.padding(.horizontal, 22).frame(height: 52)
        }
        .background(canvasColor).tint(Color(red: 0.33, green: 0.48, blue: 0.88))
        .preferredColorScheme(model.scheme)
        .alert("Discard unsaved preview changes?", isPresented: $discard) {
            Button("Keep editing", role: .cancel) {}
            Button("Discard changes", role: .destructive) { model.revert() }
        } message: { Text("This restores the last saved preview. Your physical mouse is unaffected.") }
        .alert("Preview error", isPresented: Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })) {
            Button("OK") { model.error = nil }
        } message: { Text(model.error ?? "") }
    }

    var profilePicker: some View {
        Picker("Profile", selection: $model.profile) {
            ForEach(Array(model.data.profiles.enumerated()), id: \.element.id) { index, profile in Text(profile.name).tag(index) }
        }.frame(maxWidth: 250)
    }

    var heading: some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: 6) {
                Text(model.page == .buttons ? "MX Master 3S" : model.page.rawValue).font(.system(size: 25, weight: .semibold))
                Text(model.direction.subtitle).font(.subheadline).foregroundStyle(.secondary)
            }
            Spacer()
            profilePicker
        }
    }

    var sidebar: some View {
        VStack(alignment: .leading, spacing: 5) {
            Label("SuperLight", systemImage: "computermouse.fill").font(.system(size: 15, weight: .semibold)).padding(.vertical, 16).padding(.horizontal, 8)
            Text("WORKSPACE").font(.system(size: 9, weight: .semibold)).tracking(1).foregroundStyle(.secondary).padding(.horizontal, 10).padding(.top, 14).padding(.bottom, 8)
            ForEach(Page.allCases, id: \.self) { page in
                Button { model.page = page } label: {
                    Label(page.rawValue, systemImage: page.icon).font(.system(size: 12, weight: model.page == page ? .medium : .regular))
                        .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 10).frame(height: 40)
                        .background(model.page == page ? Color.primary.opacity(0.075) : .clear, in: RoundedRectangle(cornerRadius: 7))
                        .contentShape(Rectangle())
                }.buttonStyle(.plain).accessibilityAddTraits(model.page == page ? .isSelected : [])
            }
            Spacer()
            Divider().padding(.bottom, 10)
            Label("Preview device", systemImage: "cube.transparent").font(.caption.weight(.medium))
            Text("No hardware connection").font(.caption2).foregroundStyle(.secondary)
        }.padding(.horizontal, 12).padding(.bottom, 22).frame(width: 178)
            .background(scheme == .dark ? Color(white: 0.065) : Color(white: 0.94))
    }

    var deviceNotice: some View {
        HStack(spacing: 10) {
            Image(systemName: model.deviceState == "Preview device" ? "cube.transparent" : "exclamationmark.circle")
            Text(model.deviceState == "Preview device" ? "MX Master sample · explore without changing your mouse" : model.deviceState == "Disconnected" ? "Mouse disconnected · reconnect to apply settings" : "Input permissions required · settings remain available offline")
                .font(.caption).foregroundStyle(.secondary)
            Spacer()
        }.padding(.vertical, 10)
    }

    var workbench: some View {
        VStack(spacing: 0) {
            HStack {
                Label("SuperLight", systemImage: "computermouse.fill").font(.headline)
                Spacer()
                Picker("Page", selection: $model.page) { ForEach(Page.allCases, id: \.self) { Text($0.rawValue) } }
                    .pickerStyle(.segmented).frame(width: 450)
                Spacer()
            }.padding(.horizontal, 32).padding(.vertical, 16)
            Divider()
            if model.page == .buttons {
                ScrollView {
                    VStack(alignment: .leading, spacing: 12) {
                        heading
                        deviceNotice
                        MouseDiagram(model: model, mouseImage: mouseImage).frame(height: 320)
                        HStack(spacing: 6) {
                            ForEach(0..<7) { index in
                                Button { model.selected = index } label: {
                                    Text(controlNames[index]).font(.system(size: 11, weight: .medium)).padding(.horizontal, 8).frame(height: 38)
                                        .background(model.selected == index ? Color.accentColor.opacity(0.18) : .clear, in: Capsule())
                                }.buttonStyle(.plain)
                            }
                        }.padding(5).modifier(GlassControl()).frame(maxWidth: .infinity)
                        HStack(alignment: .top, spacing: 28) {
                            VStack(alignment: .leading, spacing: 8) {
                                Text("MAKE IT YOURS").font(.system(size: 10, weight: .semibold)).tracking(1).foregroundStyle(.secondary)
                                Text("One button.\nYour next action.").font(.system(size: 22, weight: .medium))
                            }.frame(width: 200, alignment: .leading)
                            ActionEditor(model: model)
                        }.padding(22).modifier(Surface()).padding(.top, 10)
                    }.padding(32)
                }
            } else { supportingPage.padding(32) }
        }.frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    var inspector: some View {
        HStack(spacing: 0) {
            sidebar
            Divider()
            if model.page == .buttons {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        Text("Your mouse").font(.system(size: 25, weight: .semibold))
                        Text("MX Master 3S").foregroundStyle(.secondary)
                        MouseDiagram(model: model, mouseImage: mouseImage).frame(height: 275)
                        Text("CONTROLS").font(.system(size: 10, weight: .semibold)).tracking(1).foregroundStyle(.secondary)
                        ButtonList(model: model)
                    }.padding(24)
                }.frame(maxWidth: .infinity)
                Divider()
                ScrollView {
                    VStack(alignment: .leading, spacing: 26) {
                        Text("INSPECTOR").font(.system(size: 10, weight: .semibold)).tracking(1).foregroundStyle(.secondary)
                        profilePicker
                        Divider()
                        ActionEditor(model: model)
                        Spacer(minLength: 50)
                        Label("Preview only", systemImage: "cube.transparent").font(.caption).foregroundStyle(.secondary)
                        Text("Select a numbered point or a row. This panel follows your selection.").font(.caption).foregroundStyle(.secondary)
                    }.padding(24)
                }.frame(width: 300).background(scheme == .dark ? Color(white: 0.105) : .white)
            } else { supportingPage.padding(28) }
        }.frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    var commandCenter: some View {
        HStack(spacing: 0) {
            sidebar
            Divider()
            if model.page == .buttons {
                ScrollView {
                    VStack(alignment: .leading, spacing: 22) {
                        heading
                        HStack(spacing: 18) {
                            Image(nsImage: mouseImage).resizable().scaledToFit().frame(width: 130, height: 85).accessibilityLabel("MX Master mouse")
                            VStack(alignment: .leading, spacing: 6) {
                                Text("A shortcut for every move.").font(.headline)
                                Text("Select a row to edit its assignment.").font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                        }.padding(12).modifier(Surface())
                        HStack {
                            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                            TextField("Filter buttons or assigned actions", text: $filter).textFieldStyle(.plain).accessibilityLabel("Filter buttons")
                            if !filter.isEmpty { Button { filter = "" } label: { Image(systemName: "xmark.circle.fill") }.buttonStyle(.plain).accessibilityLabel("Clear filter") }
                        }.padding(12).modifier(Surface())
                        HStack { Text("BUTTON"); Spacer(); Text("ASSIGNMENT") }.font(.system(size: 10, weight: .medium)).tracking(0.8).foregroundStyle(.secondary).padding(.horizontal, 12)
                        ButtonList(model: model, filter: filter, inlineActions: true)
                        Divider()
                        ActionEditor(model: model).padding(20).modifier(Surface())
                    }.padding(28)
                }
            } else { supportingPage.padding(28) }
        }.frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ViewBuilder var supportingPage: some View {
        VStack(alignment: .leading, spacing: 22) {
            heading
            switch model.page {
            case .scroll: ScrollSettings(model: model)
            case .profiles: ProfileSettings(model: model)
            case .settings: GeneralSettings(model: model)
            case .buttons: EmptyView()
            }
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}

struct ScrollSettings: View {
    @ObservedObject var model: PreviewModel
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                VStack(alignment: .leading, spacing: 16) {
                    Text("Pointer speed").font(.headline)
                    HStack { Text("Sensitivity"); Spacer(); Text("\(Int(model.data.dpi)) DPI").monospacedDigit().foregroundStyle(.secondary) }
                    Slider(value: $model.data.dpi, in: 200...8000, step: 50) { Text("Pointer DPI") }
                    HStack { Text("Precise"); Spacer(); Text("Fast") }.font(.caption).foregroundStyle(.secondary)
                    HStack { ForEach([800, 1000, 1600, 2400], id: \.self) { value in Button("\(value)") { model.data.dpi = Double(value) } } }
                }.padding(22).modifier(Surface())
                VStack(alignment: .leading, spacing: 18) {
                    Text("Scroll wheel").font(.headline)
                    Toggle("SmartShift", isOn: $model.data.smartShift).toggleStyle(.switch)
                    Text("Automatically switch between precise ratchet steps and free spin.").font(.caption).foregroundStyle(.secondary)
                    Slider(value: $model.data.threshold, in: 1...50, step: 1) { Text("SmartShift threshold") }.disabled(!model.data.smartShift)
                    Divider()
                    Toggle("Invert vertical scrolling", isOn: $model.data.verticalInverted)
                    Toggle("Invert horizontal scrolling", isOn: $model.data.horizontalInverted)
                    Toggle("Leave trackpad scrolling unchanged", isOn: $model.data.ignoreTrackpad)
                }.padding(22).modifier(Surface())
                Text("Preview values only. These controls do not write to the mouse.").font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}

struct ProfileSettings: View {
    @ObservedObject var model: PreviewModel
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
                Button("Add profile", systemImage: "plus") { adding = true }.disabled(model.data.profiles.count >= 16)
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
                Text("Application matching is simulated in this preview.").font(.caption).foregroundStyle(.secondary)
                HStack { Button("Cancel") { adding = false }.keyboardShortcut(.cancelAction); Spacer(); Button("Create profile") {
                    if model.addProfile(name: name, application: application) { name = ""; application = ""; adding = false }
                }.keyboardShortcut(.defaultAction).disabled(name.trimmingCharacters(in: .whitespaces).isEmpty) }
            }.padding(24).frame(width: 410).onAppear { nameFocused = true }
        }
        .alert("Delete this preview profile?", isPresented: $deleting) {
            Button("Cancel", role: .cancel) {}
            Button("Delete", role: .destructive) { model.deleteProfile() }
        } message: { Text("Its preview button assignments will be removed. You can revert before saving.") }
    }
}

struct GeneralSettings: View {
    @ObservedObject var model: PreviewModel
    var body: some View {
        VStack(alignment: .leading, spacing: 22) {
            VStack(alignment: .leading, spacing: 18) {
                Text("Appearance").font(.headline)
                Picker("Theme", selection: $model.theme) { ForEach(["Light", "Dark", "System"], id: \.self) { Text($0) } }.pickerStyle(.segmented)
                Text("System follows macOS. Light and dark use the same layout and controls.").font(.caption).foregroundStyle(.secondary)
            }.padding(22).modifier(Surface())
            VStack(alignment: .leading, spacing: 18) {
                Text("Preview scenarios").font(.headline)
                Picker("Device state", selection: $model.deviceState) { ForEach(["Preview device", "Disconnected", "Permissions required"], id: \.self) { Text($0) } }
                Toggle("Start at login", isOn: $model.data.startAtLogin)
                Text("This only edits preview data. It does not register a login item or request permissions.").font(.caption).foregroundStyle(.secondary)
                Text("State: \(model.deviceState)").font(.caption.weight(.medium))
            }.padding(22).modifier(Surface())
            Text("Native SwiftUI · Original Mouser artwork · Local preview storage\nNo Rust service, HID access, telemetry or cloud connection.").font(.caption).foregroundStyle(.secondary).lineSpacing(5)
            Spacer()
        }
    }
}
