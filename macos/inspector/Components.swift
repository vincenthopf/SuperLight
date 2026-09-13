import SwiftUI
import AppKit

struct MouseDiagram: View {
    @ObservedObject var model: ServiceModel
    var showHotspots = true
    let mouseImage: NSImage
    let points: [(Int, CGFloat, CGFloat)] = [(0, 0.33, 0.45), (1, 0.70, 0.63), (2, 0.65, 0.40), (3, 0.60, 0.48), (4, 0.59, 0.345), (6, 0.43, 0.25)]

    var body: some View {
        GeometryReader { geometry in
            let aspect = mouseImage.size.width / mouseImage.size.height
            let width = min(geometry.size.width, geometry.size.height * aspect)
            let height = width / aspect
            let left = (geometry.size.width - width) / 2
            let top = (geometry.size.height - height) / 2
            ZStack(alignment: .topLeading) {
                Image(nsImage: mouseImage).resizable().interpolation(.high)
                    .frame(width: width, height: height).position(x: geometry.size.width / 2, y: geometry.size.height / 2)
                    .accessibilityLabel("MX Master mouse illustration from Mouser")
                if showHotspots && (model.device["layout_key"] as? String ?? "").contains("master") {
                    ForEach(points, id: \.0) { point in
                        let active = model.selected == point.0 || (point.0 == 4 && model.selected == 5)
                        Button { model.selected = point.0 } label: {
                            ZStack {
                                Circle().fill(active ? Color.accentColor : Color.black.opacity(0.75)).frame(width: active ? 30 : 24, height: active ? 30 : 24)
                                Circle().strokeBorder(.white.opacity(0.9), lineWidth: active ? 2 : 1).frame(width: active ? 30 : 24, height: active ? 30 : 24)
                                Text("\(point.0 + 1)").font(.system(size: 11, weight: .bold)).foregroundStyle(.white)
                            }.frame(width: 44, height: 44).contentShape(Circle())
                        }
                        .buttonStyle(.plain)
                        .disabled(!model.supportedButton(point.0))
                        .accessibilityLabel("Select \(controlNames[point.0])")
                        .accessibilityAddTraits(active ? .isSelected : [])
                        .help("\(controlNames[point.0]) · \(model.data.profiles[model.profile].actions[point.0])")
                        .position(x: left + point.1 * width, y: top + point.2 * height)
                    }
                }
            }
        }
    }
}

struct ButtonList: View {
    @ObservedObject var model: ServiceModel
    var body: some View {
        List(selection: $model.selected) {
            Section("Controls") {
                ForEach(0..<controlNames.count, id: \.self) { index in
                    if model.supportedButton(index) {
                        HStack {
                            Text("\(index + 1)").font(.caption.monospacedDigit()).foregroundStyle(.secondary).frame(width: 24)
                            Text(controlNames[index])
                            Spacer()
                            Text(model.data.profiles[model.profile].actions[index]).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }.tag(index).padding(.vertical, 4)
                    }
                }
            }
        }.listStyle(.inset)
    }
}

struct ActionEditor: View {
    @ObservedObject var model: ServiceModel
    @State private var pickerOpen = false
    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack {
                Image(systemName: (7...10).contains(model.selected) || model.selected == 1 ? "hand.draw" : "cursorarrow.click")
                    .font(.title3).frame(width: 40, height: 40).background(.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 10))
                VStack(alignment: .leading, spacing: 4) {
                    Text(controlNames[model.selected]).font(.headline)
                    Text(model.data.profiles[model.profile].name).font(.caption).foregroundStyle(.secondary)
                }
            }
            Picker("Action", selection: Binding(get: { model.action }, set: { model.assign($0) })) {
                ForEach(model.actions, id: \.self) { action in Text(action[1]).tag(action[1]) }
                if !model.actions.contains(where: { $0[1] == model.action }) { Text(model.action).tag(model.action) }
            }
            Button("Search actions or set shortcut…") { pickerOpen = true }
            Text("Changes apply to this profile only. The other buttons keep their assignments.")
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            if model.selected == 1 || (7...10).contains(model.selected) {
                Divider()
                Text("Hold and move").font(.subheadline.weight(.medium))
                HStack {
                    ForEach(7..<11) { index in
                        Button { model.selected = index } label: {
                            Image(systemName: ["arrow.left", "arrow.right", "arrow.up", "arrow.down"][index - 7]).frame(width: 36, height: 36)
                        }.help(controlNames[index]).accessibilityLabel(controlNames[index])
                    }
                }
            }
            Button("Use default behavior") { model.assign("Pass through") }.buttonStyle(.link)
        }
        .sheet(isPresented: $pickerOpen) { ActionChooser(model: model) }
    }
}

struct ActionChooser: View {
    @ObservedObject var model: ServiceModel
    @Environment(\.dismiss) var dismiss
    @State private var query = ""
    @State private var shortcut = ""
    @State private var validation = ""
    @FocusState private var searchFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack { Text("Choose an action").font(.title2.weight(.semibold)); Spacer(); Button("Done") { dismiss() }.keyboardShortcut(.cancelAction) }
            Text("\(controlNames[model.selected]) · \(model.data.profiles[model.profile].name)").foregroundStyle(.secondary)
            TextField("Search actions", text: $query).textFieldStyle(.roundedBorder).focused($searchFocused).accessibilityLabel("Search actions")
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    ForEach(model.actionGroups, id: \.0) { group in
                        let matches = group.1.filter { query.isEmpty || $0.localizedCaseInsensitiveContains(query) }
                        if !matches.isEmpty {
                            Text(group.0.uppercased()).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                            ForEach(matches, id: \.self) { action in
                                Button { model.assign(action); dismiss() } label: {
                                    HStack { Text(action); Spacer(); if model.action == action { Image(systemName: "checkmark").foregroundStyle(Color.accentColor) } }.frame(minHeight: 32).contentShape(Rectangle())
                                }.buttonStyle(.plain)
                            }
                        }
                    }
                    if !query.isEmpty && !model.actionGroups.flatMap(\.1).contains(where: { $0.localizedCaseInsensitiveContains(query) }) {
                        Text("No matching actions. Try “tab” or “volume”.").foregroundStyle(.secondary)
                    }
                }.padding(4)
            }.frame(height: 250)
            Divider()
            Text("Custom shortcut").font(.headline)
            HStack {
                TextField("cmd+shift+p", text: $shortcut).textFieldStyle(.roundedBorder).accessibilityLabel("Custom shortcut")
                    .onSubmit { useShortcut() }
                Button("Use shortcut") { useShortcut() }.disabled(shortcut.isEmpty)
            }
            Text(validation.isEmpty ? "Use cmd, ctrl, alt or shift, followed by a key. Saved shortcuts run through the Rust service." : validation)
                .font(.caption).foregroundStyle(validation.isEmpty ? Color.secondary : Color.red)
        }.padding(24).frame(width: 430).onAppear { searchFocused = true }
    }

    func useShortcut() {
        let normalized = shortcut.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        do {
            _ = try NativeBridge.call(["local": "validate_action", "action": "custom:" + normalized])
            model.assign("custom:" + normalized)
            dismiss()
        } catch { validation = error.localizedDescription }
    }
}
