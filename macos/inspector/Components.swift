import SwiftUI
import AppKit

struct Surface: ViewModifier {
    @Environment(\.colorScheme) var scheme
    func body(content: Content) -> some View {
        content.background(scheme == .dark ? Color(white: 0.115) : Color.white, in: RoundedRectangle(cornerRadius: 14))
            .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(.primary.opacity(0.09), lineWidth: 0.5))
    }
}

struct GlassControl: ViewModifier {
    @Environment(\.accessibilityReduceTransparency) var reduceTransparency
    func body(content: Content) -> some View {
        if reduceTransparency { content.background(.background, in: Capsule()) }
        else { content.glassEffect(.regular, in: .capsule) }
    }
}

struct MouseDiagram: View {
    @ObservedObject var model: ServiceModel
    var showHotspots = true
    let mouseImage: NSImage
    let points: [(Int, CGFloat, CGFloat)] = [(0, 0.33, 0.45), (1, 0.70, 0.63), (2, 0.65, 0.40), (3, 0.60, 0.48), (4, 0.59, 0.345), (6, 0.43, 0.25)]

    var body: some View {
        GeometryReader { geometry in
            let width = min(geometry.size.width, geometry.size.height * 1408 / 768)
            let height = width * 768 / 1408
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
    var filter = ""
    var inlineActions = false
    var body: some View {
        VStack(spacing: 2) {
            ForEach(0..<controlNames.count, id: \.self) { index in
                let action = model.data.profiles[model.profile].actions[index]
                if model.supportedButton(index) && (filter.isEmpty || controlNames[index].localizedCaseInsensitiveContains(filter) || action.localizedCaseInsensitiveContains(filter)) {
                    Button { model.selected = index } label: {
                        HStack(spacing: 12) {
                            Text("\(index + 1)").font(.system(size: 11, weight: .medium, design: .monospaced))
                                .foregroundStyle(.secondary).frame(width: 24, height: 24)
                                .background(.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 6))
                            Text(controlNames[index]).fontWeight(model.selected == index ? .medium : .regular)
                            Spacer(minLength: 8)
                            if inlineActions { Text(action).foregroundStyle(.secondary).lineLimit(1) }
                            Image(systemName: "chevron.right").font(.system(size: 9, weight: .semibold)).foregroundStyle(.secondary)
                        }
                        .padding(.horizontal, 10).frame(minHeight: 44)
                        .background(model.selected == index ? Color.accentColor.opacity(0.14) : .clear, in: RoundedRectangle(cornerRadius: 8))
                        .contentShape(Rectangle())
                    }.buttonStyle(.plain).accessibilityAddTraits(model.selected == index ? .isSelected : [])
                }
            }
            if !filter.isEmpty && !(0..<7).contains(where: { controlNames[$0].localizedCaseInsensitiveContains(filter) || model.data.profiles[model.profile].actions[$0].localizedCaseInsensitiveContains(filter) }) {
                VStack(spacing: 8) { Image(systemName: "magnifyingglass"); Text("No matches for \"\(filter)\"").font(.caption).foregroundStyle(.secondary) }.frame(height: 160)
            }
        }
    }
}

struct ActionEditor: View {
    @ObservedObject var model: ServiceModel
    @State private var pickerOpen = false
    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack {
                Image(systemName: model.selected >= 7 || model.selected == 1 ? "hand.draw" : "cursorarrow.click")
                    .font(.title3).frame(width: 40, height: 40).background(.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 10))
                VStack(alignment: .leading, spacing: 4) {
                    Text(controlNames[model.selected]).font(.headline)
                    Text(model.data.profiles[model.profile].name).font(.caption).foregroundStyle(.secondary)
                }
            }
            Text("ASSIGNED ACTION").font(.system(size: 10, weight: .semibold)).tracking(1).foregroundStyle(.secondary)
            Button { pickerOpen = true } label: {
                HStack { Text(model.action).lineLimit(2); Spacer(); Image(systemName: "chevron.up.chevron.down").font(.caption) }
                    .padding(12).frame(minHeight: 44).background(.primary.opacity(0.045), in: RoundedRectangle(cornerRadius: 8)).contentShape(Rectangle())
            }.buttonStyle(.plain).help("Choose an action for \(controlNames[model.selected])")
            Text("Changes apply to this profile only. The other buttons keep their assignments.")
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            if model.selected == 1 || model.selected >= 7 {
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
        let parts = shortcut.lowercased().split(separator: "+", omittingEmptySubsequences: false).map { $0.trimmingCharacters(in: .whitespaces) }
        let modifiers = Set(["cmd", "ctrl", "alt", "shift"])
        guard parts.count >= 2, parts.count <= 5, Set(parts.dropLast()).count == parts.count - 1,
              parts.dropLast().allSatisfy({ modifiers.contains($0) }), let key = parts.last,
              key.count == 1 && key.first!.isLetter || ["return", "space", "tab", "left", "right", "up", "down"].contains(key) else {
            validation = "Use modifiers and one key, for example cmd+shift+p."
            return
        }
        model.assign("custom:" + parts.joined(separator: "+"))
        dismiss()
    }
}
