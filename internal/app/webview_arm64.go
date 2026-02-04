//go:build !windows || (windows && arm64)

package app

// LoginLegendaryAuto is a stub for Windows ARM64 where webview_go is not supported
func (a *App) EpicLoginAuto() string {
	return "Auto login is not supported on this platform (or Windows ARM64). Please use manual login method."
}
