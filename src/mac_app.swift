import Cocoa
import WebKit

class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    var window: NSWindow!
    var webView: WKWebView!
    var serverProcess: Process?
    var retryCount = 0
    let maxRetries = 50 // 50 * 200ms = 10s
    let targetURL = URL(string: "http://127.0.0.1:4242")!
    let healthURL = URL(string: "http://127.0.0.1:4242/api/health")!
    var didLaunchServer = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        let rect = NSRect(x: 0, y: 0, width: 1280, height: 820)
        window = NSWindow(contentRect: rect,
                          styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                          backing: .buffered, defer: false)
        window.titlebarAppearsTransparent = true
        window.title = "swch Hub"
        window.center()
        window.isMovableByWindowBackground = true
        window.delegate = self
        
        let config = WKWebViewConfiguration()
        webView = WKWebView(frame: rect, configuration: config)
        webView.autoresizingMask = [.width, .height]
        window.contentView = webView
        
        showLoadingScreen()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)

        checkAndStartBackend()
    }

    func showLoadingScreen() {
        let html = """
        <!DOCTYPE html>
        <html>
        <head>
          <meta charset="utf-8">
          <style>
            body { margin: 0; background: #111214; color: #f2f2f4; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; display: grid; min-height: 100vh; place-items: center; user-select: none; }
            .card { text-align: center; }
            .spinner { width: 36px; height: 36px; border: 3px solid rgba(255,100,45,0.2); border-top-color: #ff642d; border-radius: 50%; animation: spin 0.8s linear infinite; margin: 0 auto 16px; }
            @keyframes spin { to { transform: rotate(360deg); } }
            h2 { margin: 0 0 8px; font-size: 20px; font-weight: 600; }
            p { margin: 0; color: #888; font-size: 13px; }
          </style>
        </head>
        <body>
          <div class="card">
            <div class="spinner"></div>
            <h2>Запуск SWCH...</h2>
            <p>Инициализация игрового пространства</p>
          </div>
        </body>
        </html>
        """
        webView.loadHTMLString(html, baseURL: nil)
    }

    func checkAndStartBackend() {
        checkHealth { [weak self] isHealthy in
            guard let self = self else { return }
            if isHealthy {
                DispatchQueue.main.async {
                    self.webView.load(URLRequest(url: self.targetURL))
                }
            } else {
                if !self.didLaunchServer {
                    self.launchServerProcess()
                }
                self.scheduleNextHealthCheck()
            }
        }
    }

    func scheduleNextHealthCheck() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in
            guard let self = self else { return }
            self.retryCount += 1
            if self.retryCount >= self.maxRetries {
                self.showErrorScreen()
                return
            }
            self.checkHealth { [weak self] isHealthy in
                guard let self = self else { return }
                if isHealthy {
                    DispatchQueue.main.async {
                        self.webView.load(URLRequest(url: self.targetURL))
                    }
                } else {
                    self.scheduleNextHealthCheck()
                }
            }
        }
    }

    func checkHealth(completion: @escaping (Bool) -> Void) {
        var req = URLRequest(url: healthURL)
        req.timeoutInterval = 1.0
        let task = URLSession.shared.dataTask(with: req) { _, resp, err in
            if err == nil, let http = resp as? HTTPURLResponse, http.statusCode == 200 {
                completion(true)
            } else {
                completion(false)
            }
        }
        task.resume()
    }

    func launchServerProcess() {
        didLaunchServer = true
        let bundleURL = Bundle.main.bundleURL
        let homeDir = FileManager.default.homeDirectoryForCurrentUser
        let candidates = [
            bundleURL.appendingPathComponent("Contents/MacOS/swch-server"),
            bundleURL.appendingPathComponent("Contents/MacOS/swch_backend"),
            bundleURL.deletingLastPathComponent().appendingPathComponent("swch-server"),
            bundleURL.deletingLastPathComponent().appendingPathComponent("swch"),
            URL(fileURLWithPath: "/Applications/swch.app/Contents/MacOS/swch-server"),
            homeDir.appendingPathComponent("swch2/target_build/release/swch"),
            homeDir.appendingPathComponent("swch2/target_build/debug/swch")
        ]

        guard let serverExe = candidates.first(where: { FileManager.default.isExecutableFile(atPath: $0.path) }) else {
            NSLog("swch: no server binary candidate found")
            return
        }

        let proc = Process()
        proc.executableURL = serverExe
        proc.environment = ProcessInfo.processInfo.environment
        do {
            try proc.run()
            self.serverProcess = proc
            NSLog("swch: started backend at %@", serverExe.path)
        } catch {
            NSLog("swch: failed to launch server: %@", error.localizedDescription)
        }
    }

    func showErrorScreen() {
        let html = """
        <!DOCTYPE html>
        <html>
        <head>
          <meta charset="utf-8">
          <style>
            body { margin: 0; background: #111214; color: #f2f2f4; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; display: grid; min-height: 100vh; place-items: center; }
            .box { max-width: 440px; padding: 32px; border: 1px solid #36373d; border-radius: 16px; background: #1b1c20; text-align: center; }
            h2 { margin: 0 0 12px; font-size: 22px; }
            p { margin: 0 0 20px; color: #adb0bb; font-size: 14px; line-height: 1.5; }
            button { background: #ff642d; border: none; color: #111; font-weight: 700; padding: 10px 22px; border-radius: 8px; cursor: pointer; font-size: 14px; }
          </style>
        </head>
        <body>
          <div class="box">
            <h2>Не удалось запустить SWCH</h2>
            <p>Серверный движок не ответил вовремя. Проверьте запуск приложения или повторите попытку.</p>
            <button onclick="location.reload()">Повторить попытку</button>
          </div>
        </body>
        </html>
        """
        webView.loadHTMLString(html, baseURL: nil)
    }

    func applicationWillTerminate(_ notification: Notification) {
        if let proc = serverProcess, proc.isRunning {
            proc.terminate()
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        return true
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let delegate = AppDelegate()
app.delegate = delegate
app.run()
