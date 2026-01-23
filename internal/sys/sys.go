package sys

import (
	"fmt"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	"golang.org/x/sys/windows/registry"
)

func GetSteamPath() (string, error) {
	k, err := registry.OpenKey(registry.CURRENT_USER, `Software\Valve\Steam`, registry.QUERY_VALUE)
	if err != nil { return "", err }
	defer k.Close()
	path, _, err := k.GetStringValue("SteamPath")
	if err != nil { return "", err }
	return filepath.Clean(strings.ReplaceAll(path, "/", "\\")), nil
}

func KillSteam() {
	exec.Command("taskkill", "/F", "/IM", "steam.exe").Run()
	exec.Command("taskkill", "/F", "/IM", "steamwebhelper.exe").Run()
	time.Sleep(3 * time.Second)
}

func SetSteamUser(username string) error {
	if username == "" { return fmt.Errorf("empty username") }
	k, _, err := registry.CreateKey(registry.CURRENT_USER, `Software\Valve\Steam`, registry.SET_VALUE)
	if err != nil { return err }
	defer k.Close()
	
	if err := k.SetStringValue("AutoLoginUser", username); err != nil { return err }
	if err := k.SetDWordValue("RememberPassword", 1); err != nil { return err }
	k.SetDWordValue("SkipOfflineModeWarning", 1) 
	return nil
}

func StartGame(pathOrUrl string) {
	cmd := exec.Command("cmd", "/C", "start", "", pathOrUrl)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	cmd.Start()
}

func StartGameWithArgs(exePath string, args ...string) {
	cmd := exec.Command(exePath, args...)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	cmd.Start()
}