import Cocoa
import WebKit

// Isolated, temporary Steam login window. Never exports cookies or stores the key.
class KeyWindow: NSObject, NSApplicationDelegate, NSWindowDelegate, WKNavigationDelegate {
    var window: NSWindow!
    var web: WKWebView!
    var timer: Timer?
    var reading = false
    var finished = false
    let expected = CommandLine.arguments.dropFirst().first ?? ""
    func applicationDidFinishLaunching(_ notification: Notification) {
        guard expected.range(of: "^[0-9]{17}$", options: .regularExpression) != nil else { finish(nil); return }
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        web = WKWebView(frame: .zero, configuration: config)
        web.navigationDelegate = self
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1050, height: 760),
                          styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = "SWCH — вход в аккаунт \(expected)"
        window.contentView = web
        window.delegate = self
        window.center()
        window.makeKeyAndOrderFront(nil)
        web.load(URLRequest(url: URL(string: "https://steamcommunity.com/dev/apikey?l=english")!))
        NSApp.activate(ignoringOtherApps: true)
        timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in self?.readKey() }
        DispatchQueue.main.asyncAfter(deadline: .now() + 300) { [weak self] in self?.finish(nil) }
    }
    func readKey() {
        guard !reading, !finished, let url = web.url,
              url.scheme == "https", url.host == "steamcommunity.com", url.path == "/dev/apikey" else { return }
        reading = true
        // Only the key's labelled paragraph on the official page, never arbitrary hex in scripts.
        web.evaluateJavaScript("""
        (() => {
          if (location.protocol !== 'https:' || location.hostname !== 'steamcommunity.com' || location.pathname !== '/dev/apikey') return null;
          if (String(window.g_steamID || '') !== '\(expected)') return 'wrong-account';
          for (const p of document.querySelectorAll('#bodyContents_ex p')) {
            const match = p.textContent.trim().match(/^Key:\\s*([A-Fa-f0-9]{32})$/);
            if (match) return match[1];
          }
          return null;
        })()
        """) { [weak self] result, _ in
            guard let self = self else { return }
            self.reading = false
            if result as? String == "wrong-account" {
                self.window.title = "Нужен аккаунт \(self.expected) — выйди из текущего и войди в нужный"
                return
            }
            if let key = result as? String, key.range(of: "^[A-Fa-f0-9]{32}$", options: .regularExpression) != nil {
                self.finish(key)
            }
        }
    }
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        guard let url = action.request.url, url.scheme == "https",
              let host = url.host,
              host == "steamcommunity.com" || host == "store.steampowered.com" || host == "login.steampowered.com" else {
            decisionHandler(.cancel); return
        }
        decisionHandler(.allow)
    }
    func finish(_ key: String?) {
        guard !finished else { return }
        finished = true
        timer?.invalidate()
        if let key = key { FileHandle.standardOutput.write(Data(key.utf8)) }
        NSApp.terminate(nil)
    }
    func windowWillClose(_ notification: Notification) { finish(nil) }
}
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let delegate = KeyWindow()
app.delegate = delegate
app.run()
