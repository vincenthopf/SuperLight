import SwiftUI
import AppKit

struct MouseDiagram: View {
    @State private var hovered: Int?
    @Bindable var model: ServiceModel
    var showHotspots = true
    let mouseImage: NSImage

    private struct Control: Identifiable {
        let id: Int
        let x: CGFloat
        let y: CGFloat
        let column: CGFloat
        let above: Bool
        let title: String
    }

    private let controls = [
        Control(id: 0, x: 0.34, y: 0.45, column: 0, above: true, title: "Middle click"),
        Control(id: 6, x: 0.43, y: 0.25, column: 1, above: true, title: "Wheel mode"),
        Control(id: 4, x: 0.59, y: 0.345, column: 2, above: true, title: "Thumb wheel"),
        Control(id: 3, x: 0.60, y: 0.48, column: 0, above: false, title: "Forward"),
        Control(id: 2, x: 0.65, y: 0.40, column: 1, above: false, title: "Back"),
        Control(id: 1, x: 0.70, y: 0.63, column: 2, above: false, title: "Thumb gesture")
    ]

    var body: some View {
        GeometryReader { geometry in
            let aspect = mouseImage.size.width / mouseImage.size.height
            let width = min(geometry.size.width, (geometry.size.height - 136) * aspect)
            let height = width / aspect
            let left = (geometry.size.width - width) / 2
            let top = (geometry.size.height - height) / 2
            ZStack(alignment: .topLeading) {
                Image(nsImage: mouseImage).resizable().interpolation(.high)
                    .frame(width: width, height: height)
                    .position(x: geometry.size.width / 2, y: geometry.size.height / 2)
                    .accessibilityLabel("Mouse control diagram")
                if showHotspots && (model.device["layout_key"] as? String ?? "").contains("master") {
                    ForEach(controls.filter { model.supportedButton($0.id) }) { control in
                        let active = model.selected == control.id || (control.id == 4 && model.selected == 5) || (control.id == 1 && (7...10).contains(model.selected))
                        let highlighted = active || hovered == control.id
                        let anchor = CGPoint(x: left + control.x * width, y: top + control.y * height)
                        let label = CGPoint(x: geometry.size.width * (control.column + 0.5) / 3, y: control.above ? 32 : geometry.size.height - 32)
                        Path { path in
                            path.move(to: anchor)
                            path.addLine(to: CGPoint(x: label.x, y: control.above ? top - 12 : top + height + 12))
                            path.addLine(to: CGPoint(x: label.x, y: label.y + (control.above ? 29 : -29)))
                        }
                        .stroke(highlighted ? Color.accentColor : Color.secondary.opacity(0.4), style: StrokeStyle(lineWidth: highlighted ? 1.5 : 1, lineCap: .round, lineJoin: .round))
                        .allowsHitTesting(false)
                        .accessibilityHidden(true)

                        Button { model.selected = control.id } label: {
                            Circle().fill(highlighted ? Color.accentColor : Color(nsColor: .windowBackgroundColor))
                                .frame(width: 10, height: 10)
                                .overlay(Circle().strokeBorder(highlighted ? Color.white : Color.secondary, lineWidth: 1.5))
                                .frame(width: 32, height: 32).contentShape(Circle())
                        }
                        .buttonStyle(.plain)
                        .onHover { hovered = $0 ? control.id : nil }
                        .accessibilityLabel("Select \(control.title)")
                        .accessibilityValue(actionLabel(control.id))
                        .accessibilityAddTraits(active ? .isSelected : [])
                        .position(anchor)

                        Button { model.selected = control.id } label: {
                            VStack(spacing: 3) {
                                Text(control.title).font(.callout.weight(.medium))
                                    .foregroundStyle(highlighted ? Color.accentColor : .primary)
                                Text(actionLabel(control.id)).font(.caption).foregroundStyle(.secondary)
                                    .lineLimit(2).multilineTextAlignment(.center)
                            }
                            .frame(width: max(80, geometry.size.width / 3 - 16), height: 54)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .onHover { hovered = $0 ? control.id : nil }
                        .help(control.title + ": " + actionLabel(control.id))
                        .accessibilityAddTraits(active ? .isSelected : [])
                        .position(label)
                    }
                }
            }
        }
    }

    private func actionLabel(_ index: Int) -> String {
        let actions = model.data.profiles[model.profile].actions
        if index == 4 {
            return "← " + formattedAction(actions[4]) + " · → " + formattedAction(actions[5])
        }
        return formattedAction(actions[index])
    }
}

func formattedAction(_ action: String) -> String {
    guard action.hasPrefix("custom:") else { return action }
    let keys = action.dropFirst("custom:".count).split(separator: "+")
    let modifiers = ["cmd": "⌘", "ctrl": "⌃", "alt": "⌥", "shift": "⇧"]
    return keys.map { modifiers[String($0)] ?? $0.uppercased() }.joined()
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
            if model.selected == 4 || model.selected == 5 {
                Picker("Thumb wheel direction", selection: $model.selected) {
                    Text("Scroll left").tag(4)
                    Text("Scroll right").tag(5)
                }.pickerStyle(.segmented)
            }
            Picker("Action", selection: Binding(get: { model.action }, set: { model.assign($0) })) {
                ForEach(model.actions, id: \.self) { action in Text(formattedAction(action[1])).tag(action[1]) }
                if !model.actions.contains(where: { $0[1] == model.action }) { Text(formattedAction(model.action)).tag(model.action) }
            }
            Button("Search actions or set shortcut…") { pickerOpen = true }
            if model.selected == 1 || (7...10).contains(model.selected) {
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
            Slider(value: Binding(get: { value }, set: { value = min(range.upperBound, max(range.lowerBound, ($0 / step).rounded() * step)) }), in: range)
                .accessibilityLabel(title)
                .accessibilityValue("\(Int(value)) \(unit)")
        }
    }
}

struct ProfilePicker: View {
    @Bindable var model: ServiceModel
    var body: some View {
        Picker("Profile", selection: $model.profile) {
            ForEach(Array(model.data.profiles.enumerated()), id: \.element.id) { index, profile in
                Text(profile.name).tag(index)
            }
        }.pickerStyle(.menu)
    }
}
