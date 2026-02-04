//go:build windows && !arm64

package app

import (
	"encoding/json"
	"time"

	webview "github.com/webview/webview_go"
)

// EpicLoginAuto opens an embedded browser window for automatic login
func (a *App) EpicLoginAuto() string {
	url := epicLoginURL
	resultCh := make(chan string, 1)

	// Run webview in a goroutine
	go func() {
		// debug=true allows using inspector (RMB -> Inspect)
		w := webview.New(true)
		defer w.Destroy()
		w.SetTitle("Epic Games Login - Auto")
		w.SetSize(500, 700, webview.HintNone)

		// Function that JS will call when it finds the code
		w.Bind("sendAuthData", func(data string) {
			resultCh <- data
			w.Terminate()
		})

		// JS script that polls page content
		// Epic returns JSON directly in the page body
		script := `
		setInterval(function() {
			var text = document.body.innerText;
			// Check if it looks like JSON with sid or code
			if (text.trim().startsWith('{') && (text.includes('"sid"') || text.includes('"code"'))) {
				sendAuthData(text);
			}
		}, 500);
		`
		w.Init(script)
		w.Navigate(url)
		w.Run()
	}()

	// Wait for result or timeout
	select {
	case jsonStr := <-resultCh:
		var data struct {
			SID               string `json:"sid"`
			AuthorizationCode string `json:"authorizationCode"`
		}
		json.Unmarshal([]byte(jsonStr), &data)

		if data.SID != "" {
			return a.EpicLogin(data.SID)
		}
		if data.AuthorizationCode != "" {
			return a.EpicLogin(data.AuthorizationCode)
		}
		// If no SID, try passing as is (maybe it's code)
		return a.EpicLogin(jsonStr)

	case <-time.After(3 * time.Minute):
		return "Login timed out"
	}
}
