package scanner

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"swch/internal/models"
	"time"
)

type RobloxAccountData struct {
	Name string `json:"name"`
	// We might add more metadata here later if needed
}

func getRobloxConfigDir() (string, error) {
	configDir, err := os.UserConfigDir()
	if err != nil {
		return "", err
	}
	path := filepath.Join(configDir, "swch", "roblox_accounts")
	if err := os.MkdirAll(path, 0755); err != nil {
		return "", err
	}
	return path, nil
}

func SaveCurrentRobloxAccount(name string) error {
	fmt.Printf("[Roblox] Saving account: %s\n", name)
	if name == "" {
		return fmt.Errorf("name is empty")
	}

	configDir, err := getRobloxConfigDir()
	if err != nil {
		return err
	}

	destDir := filepath.Join(configDir, name)
	if err := os.MkdirAll(destDir, 0755); err != nil {
		return err
	}

	home, _ := os.UserHomeDir()

	// 1. Save Plist
	plistPath := filepath.Join(home, "Library", "Preferences", "com.roblox.RobloxPlayer.plist")
	if _, err := os.Stat(plistPath); err == nil {
		copyFile(plistPath, filepath.Join(destDir, "com.roblox.RobloxPlayer.plist"))
	} else {
		fmt.Printf("[Roblox] Warning: plist not found at %s\n", plistPath)
	}

	// 2. Save HTTPStorages (Cookies)
	httpStoragePath := filepath.Join(home, "Library", "HTTPStorages", "com.roblox.RobloxPlayer")
	destHttpPath := filepath.Join(destDir, "HTTPStorages")
	_ = os.RemoveAll(destHttpPath) // Clean old backup

	if _, err := os.Stat(httpStoragePath); err == nil {
		copyDir(httpStoragePath, destHttpPath)
	} else {
		fmt.Printf("[Roblox] Warning: HTTPStorages not found at %s\n", httpStoragePath)
	}

	// 3. Save Metadata
	meta := RobloxAccountData{Name: name}
	data, _ := json.MarshalIndent(meta, "", "  ")
	if err := os.WriteFile(filepath.Join(destDir, "meta.json"), data, 0644); err != nil {
		return err
	}

	return nil
}

func SwitchRobloxAccount(name string) error {
	fmt.Printf("[Roblox] Switching to account: %s\n", name)

	configDir, err := getRobloxConfigDir()
	if err != nil {
		return err
	}

	accountDir := filepath.Join(configDir, name)
	if _, err := os.Stat(accountDir); os.IsNotExist(err) {
		return fmt.Errorf("account not found")
	}

	// Kill Roblox processes
	exec.Command("killall", "RobloxPlayer").Run()
	exec.Command("killall", "Roblox").Run()
	time.Sleep(500 * time.Millisecond)

	home, _ := os.UserHomeDir()

	// 1. Restore Plist
	srcPlist := filepath.Join(accountDir, "com.roblox.RobloxPlayer.plist")
	destPlist := filepath.Join(home, "Library", "Preferences", "com.roblox.RobloxPlayer.plist")

	if _, err := os.Stat(srcPlist); err == nil {
		copyFile(srcPlist, destPlist)
		// Reload prefs
		exec.Command("killall", "cfprefsd").Run()
	}

	// 2. Restore HTTPStorages
	srcHttp := filepath.Join(accountDir, "HTTPStorages")
	destHttp := filepath.Join(home, "Library", "HTTPStorages", "com.roblox.RobloxPlayer")

	if _, err := os.Stat(srcHttp); err == nil {
		_ = os.RemoveAll(destHttp)
		copyDir(srcHttp, destHttp)
	}

	return nil
}

func ScanRobloxAccounts() []models.Account {
	var accounts []models.Account
	configDir, err := getRobloxConfigDir()
	if err != nil {
		return accounts
	}

	entries, err := os.ReadDir(configDir)
	if err != nil {
		return accounts
	}

	for _, e := range entries {
		if e.IsDir() {
			metaPath := filepath.Join(configDir, e.Name(), "meta.json")
			if _, err := os.Stat(metaPath); err == nil {
				accounts = append(accounts, models.Account{
					ID:          "roblox_" + e.Name(),
					DisplayName: e.Name(),
					Username:    e.Name(),
					Platform:    "Roblox",
				})
			}
		}
	}
	return accounts
}

func DeleteRobloxAccount(name string) error {
	configDir, err := getRobloxConfigDir()
	if err != nil {
		return err
	}
	return os.RemoveAll(filepath.Join(configDir, name))
}
