import SwiftUI
import AppKit

struct MouseDiagram: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var hovered: Int?
    @Bindable var model: ServiceModel
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
                    .accessibilityLabel("Mouse control diagram")
                if showHotspots && (model.device["layout_key"] as? String ?? "").contains("master") {
                    ForEach(points, id: \.0) { point in
                        let active = model.selected == point.0 || (point.0 == 4 && model.selected == 5)
                        Button { model.selected = point.0 } label: {
                            ZStack {
                                Circle().fill(active ? Color.accentColor : Color.black.opacity(0.75)).frame(width: active ? 30 : 24, height: active ? 30 : 24)
                                Circle().strokeBorder(.white.opacity(0.9), lineWidth: active ? 2 : 1).frame(width: active ? 30 : 24, height: active ? 30 : 24)
                                Text("\(point.0 + 1)").font(.system(size: 11, weight: .bold)).foregroundStyle(.white)
                            }.frame(width: 44, height: 44).contentShape(Circle())
                                .scaleEffect(!reduceMotion && hovered == point.0 ? 1.08 : 1)
                                .opacity(hovered == point.0 ? 1 : 0.92)
                                .animation(.timingCurve(0.25, 1, 0.5, 1, duration: 0.16), value: hovered)
                        }
                        .buttonStyle(.plain)
                        .onHover { inside in hovered = inside ? point.0 : nil }
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
    @Bindable var model: ServiceModel
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
    @Bindable var model: ServiceModel
    @State private var pickerOpen = false
    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack {
                Image(systemName: (7...10).contains(model.selected) || model.selected == 1 ? "hand.draw" : "cursorarrow.click")
                    .font(.title3).frame(width: 40, height: 40).background(.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 10))
                VStack(alignment: .leading, spacing: 4) {
                    Text(controlNames[model.selected]).font(.headline)
                    Text("Button assignment").font(.caption).foregroundStyle(.secondary)
                }
            }
            Picker("Action", selection: Binding(get: { model.action }, set: { model.assign($0) })) {
                ForEach(model.actions, id: \.self) { action in Text(action[1]).tag(action[1]) }
                if !model.actions.contains(where: { $0[1] == model.action }) { Text(model.action).tag(model.action) }
            }
            Button("Search actions or set shortcut…") { pickerOpen = true }
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
    enum Mode: String, CaseIterable {
        case action = "Action", shortcut = "Keyboard shortcut"
    }
    @Bindable var model: ServiceModel
    @Environment(\.dismiss) var dismiss
    @State private var mode = Mode.action
    @State private var query = ""
    @State private var selection: String?
    @State private var shortcut = ""
    @State private var validation = ""
    @FocusState private var focused: Mode?

    var matches: [String] {
        model.actions.map { $0[1] }.filter { query.isEmpty || $0.localizedCaseInsensitiveContains(query) }
    }
    var canAssign: Bool {
        mode == .action ? selection.map { matches.contains($0) } == true : !shortcut.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            VStack(alignment: .leading, spacing: 5) {
                Text("Assign \(controlNames[model.selected].lowercased())").font(.title2.weight(.semibold))
                Text(model.data.profiles[model.profile].name).foregroundStyle(.secondary)
            }
            Picker("Assignment type", selection: $mode) {
                ForEach(Mode.allCases, id: \.self) { Text($0.rawValue).tag($0) }
            }.pickerStyle(.segmented)
            Group {
                switch mode {
                case .action:
                    VStack(spacing: 12) {
                        TextField("Search actions", text: $query).textFieldStyle(.roundedBorder).focused($focused, equals: .action).accessibilityLabel("Search actions")
                            .onSubmit { if canAssign { assign() } }
                        List(matches, id: \.self, selection: $selection) { action in
                            HStack {
                                Text(action)
                                Spacer()
                                if model.action == action { Image(systemName: "checkmark").accessibilityLabel("Current assignment") }
                            }.padding(.vertical, 3).tag(action)
                        }
                        .listStyle(.inset)
                        .overlay {
                            if matches.isEmpty {
                                ContentUnavailableView.search(text: query)
                            }
                        }
                        .accessibilityLabel("Available actions")
                    }
                case .shortcut:
                    VStack(alignment: .leading, spacing: 10) {
                        Text("Keyboard shortcut").font(.headline)
                        TextField("cmd+shift+p", text: $shortcut).textFieldStyle(.roundedBorder).focused($focused, equals: .shortcut)
                            .accessibilityLabel("Keyboard shortcut")
                            .onSubmit { if canAssign { assign() } }
                        Text("Combine cmd, ctrl, alt or shift with a key. For example: cmd+shift+p.")
                            .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                        Spacer()
                    }
                }
            }.frame(height: 280)
            if !validation.isEmpty {
                Label(validation, systemImage: "exclamationmark.circle.fill").font(.callout).foregroundStyle(.red)
            }
            Divider()
            HStack {
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Spacer()
                Button("Assign action") { assign() }.keyboardShortcut(.defaultAction).buttonStyle(.borderedProminent).disabled(!canAssign)
            }
        }
        .padding(24)
        .frame(width: 460)
        .onAppear {
            if model.action.hasPrefix("custom:") {
                mode = .shortcut
                shortcut = String(model.action.dropFirst("custom:".count))
            } else {
                selection = model.action
            }
            focused = mode
        }
        .onChange(of: mode) { _, value in focused = value; validation = "" }
    }

    func assign() {
        guard canAssign else { return }
        let action: String
        switch mode {
        case .shortcut:
            action = "custom:" + shortcut.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        case .action:
            guard let selection else { return }
            action = selection
        }
        if model.assign(action) {
            dismiss()
        } else {
            validation = model.error ?? "The action could not be assigned."
            model.error = nil
        }
    }
}

struct SettingSlider: View {
    let title: String
    @Binding var value: Double
    let range: ClosedRange<Double>
    let step: Double
    var unit = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            LabeledContent(title) {
                Text("\(Int(value))\(unit.isEmpty ? "" : " " + unit)").monospacedDigit().foregroundStyle(.secondary)
            }
            Slider(value: $value, in: range, step: step)
                .accessibilityLabel(title)
                .accessibilityValue("\(Int(value)) \(unit)")
        }
    }
}
