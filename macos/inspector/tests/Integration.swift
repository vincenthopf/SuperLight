import Foundation

@main struct Integration {
    @MainActor static func main() throws {
        guard ProcessInfo.processInfo.environment["SUPERLIGHT_CONFIG_DIR"] != nil else { fatalError("An isolated config directory is required") }
        let model = ServiceModel()
        model.actions = try NativeBridge.call(["local": "catalog"])["actions"] as! [[String]]
        let first = try NativeBridge.call(["request": ["command": "get"]])
        model.accept(first)
        precondition(model.connected && !model.dirty)
        var config = try model.makeConfig()
        config["unknown_test_field"] = ["keep": true]
        var settings = config["settings"] as! [String: Any]
        settings["future_setting"] = "keep"
        config["settings"] = settings
        let seeded = try NativeBridge.call(["expected_instance": model.snapshot["instance"]!, "request": ["command": "apply", "expected_revision": model.snapshot["revision"]!, "config": config]])
        model.accept(seeded, saved: true)
        model.selected = 2
        model.assign("Copy")
        precondition(model.dirty)
        let draft = try model.makeConfig()
        precondition((draft["unknown_test_field"] as! [String: Bool])["keep"] == true)
        precondition((draft["settings"] as! [String: Any])["future_setting"] as? String == "keep")
        let mapping = ((draft["profiles"] as! [String: [String: Any]])["default"]!["mappings"] as! [String: Any])
        precondition(mapping["xbutton1"] as? String == "copy")
        precondition(mapping["xbutton2"] as? String == "alt_tab")
        model.accept(try NativeBridge.call(["request": ["command": "get"]]))
        precondition(model.dirty && model.action == "Copy" && !model.conflicted)
        let saved = try NativeBridge.call(["expected_instance": model.snapshot["instance"]!, "request": ["command": "apply", "expected_revision": model.snapshot["revision"]!, "config": draft]])
        model.accept(saved, saved: true)
        precondition(!model.dirty && model.action == "Copy")
        model.assign("Paste")
        var external = try model.makeConfig()
        external["external"] = true
        let advanced = try NativeBridge.call(["expected_instance": model.snapshot["instance"]!, "request": ["command": "apply", "expected_revision": model.snapshot["revision"]!, "config": external]])
        model.accept(advanced)
        precondition(model.conflicted && model.dirty && !model.canSave)
        model.revert()
        precondition(!model.conflicted && !model.dirty)
        var restarted = advanced
        var snapshot = restarted["snapshot"] as! [String: Any]
        model.assign("Undo")
        snapshot["instance"] = String(repeating: "a", count: 32)
        restarted["snapshot"] = snapshot
        model.accept(restarted)
        precondition(model.conflicted)
        do {
            _ = try NativeBridge.call(["expected_instance": "stale-instance", "request": ["command": "apply", "expected_revision": 1, "config": draft]])
            fatalError("Stale instance accepted")
        } catch {}
        do {
            _ = try NativeBridge.call(["local": "validate_action", "action": "custom:invalid-key"])
            fatalError("Invalid shortcut accepted")
        } catch {}
        print("PASS: authenticated IPC, save/readback, action IDs, unknown-field preservation, dirty polling, revision conflict, restart conflict, invalid shortcut")
    }
}
