package scanner

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"swch/internal/models"
	"swch/internal/sys"
)

type EpicManifest struct {
	FormatVersion   int    `json:"FormatVersion"`
	AppName         string `json:"AppName"`
	DisplayName     string `json:"DisplayName"`
	InstallLocation string `json:"InstallLocation"`
	MainGameAppName string `json:"MainGameAppName"`
}

// EpicAccountData хранит данные для смены аккаунта
type EpicAccountData struct {
	Name         string `json:"name"`
	RegistryId   string `json:"registryId"`   // AccountId из реестра
	ConfigBackup string `json:"configBackup"` // Путь к бэкапу GameUserSettings.ini
}

func getEpicConfigDir() string {
	configDir, _ := os.UserConfigDir()
	path := filepath.Join(configDir, "swch", "epic_accounts")
	_ = os.MkdirAll(path, 0755)
	return path
}

func getEpicGameUserSettingsPath() string {
	localAppData := os.Getenv("LOCALAPPDATA")
	return filepath.Join(localAppData, "EpicGamesLauncher", "Saved", "Config", "Windows", "GameUserSettings.ini")
}

// SaveCurrentEpicAccount сохраняет текущий залогиненный аккаунт Epic
func SaveCurrentEpicAccount(name string) error {
	if name == "" {
		return fmt.Errorf("name is empty")
	}

	// 1. Получаем ID из реестра
	regId, err := sys.GetEpicAccountId()
	if err != nil {
		return fmt.Errorf("failed to get epic registry id: %v", err)
	}

	// 2. Копируем GameUserSettings.ini
	srcPath := getEpicGameUserSettingsPath()
	if _, err := os.Stat(srcPath); os.IsNotExist(err) {
		return fmt.Errorf("GameUserSettings.ini not found. Is Epic installed?")
	}

	destDir := filepath.Join(getEpicConfigDir(), name)
	os.MkdirAll(destDir, 0755)
	destConfigPath := filepath.Join(destDir, "GameUserSettings.ini")

	if err := copyFile(srcPath, destConfigPath); err != nil {
		return err
	}

	// 3. Сохраняем метаданные
	meta := EpicAccountData{
		Name:       name,
		RegistryId: regId,
	}
	data, _ := json.MarshalIndent(meta, "", "  ")
	return os.WriteFile(filepath.Join(destDir, "meta.json"), data, 0644)
}

// SwitchEpicAccount переключает аккаунт
func SwitchEpicAccount(name string) error {
	dir := filepath.Join(getEpicConfigDir(), name)
	metaPath := filepath.Join(dir, "meta.json")

	if _, err := os.Stat(metaPath); os.IsNotExist(err) {
		return fmt.Errorf("account not found")
	}

	var meta EpicAccountData
	data, _ := os.ReadFile(metaPath)
	json.Unmarshal(data, &meta)

	// 1. Убиваем Epic
	sys.KillEpic()

	// 2. Восстанавливаем реестр
	if err := sys.SetEpicAccountId(meta.RegistryId); err != nil {
		return fmt.Errorf("registry error: %v", err)
	}

	// 3. Восстанавливаем конфиг
	configPath := getEpicGameUserSettingsPath()
	storedConfig := filepath.Join(dir, "GameUserSettings.ini")

	// Удаляем текущий конфиг (на всякий случай)
	os.Remove(configPath)

	if err := copyFile(storedConfig, configPath); err != nil {
		return err
	}

	// 4. Запускаем Epic (опционально, или просто пользователь сам запустит игру)
	// sys.StartGame("com.epicgames.launcher://")
	return nil
}

func ScanEpicGames() []models.LibraryGame {
	var games []models.LibraryGame

	programData := os.Getenv("ProgramData")
	if programData == "" {
		programData = "C:\\ProgramData"
	}
	manifestPath := filepath.Join(programData, "Epic", "EpicGamesLauncher", "Data", "Manifests")

	files, err := os.ReadDir(manifestPath)
	if err != nil {
		return games
	}

	for _, f := range files {
		if strings.HasSuffix(f.Name(), ".item") {
			data, err := os.ReadFile(filepath.Join(manifestPath, f.Name()))
			if err != nil {
				continue
			}

			var manifest EpicManifest
			if err := json.Unmarshal(data, &manifest); err != nil {
				continue
			}

			games = append(games, models.LibraryGame{
				ID:                  manifest.AppName,
				Name:                manifest.DisplayName,
				Platform:            "Epic",
				IconURL:             "https://upload.wikimedia.org/wikipedia/commons/3/31/Epic_Games_logo.svg",
				ExePath:             manifest.InstallLocation,
				AvailableOnAccounts: []models.AccountStat{},
				IsInstalled:         true,
			})
		}
	}
	return games
}

func ScanEpicAccounts() []models.Account {
	var accounts []models.Account
	baseDir := getEpicConfigDir()

	entries, err := os.ReadDir(baseDir)
	if err != nil {
		return accounts
	}

	for _, e := range entries {
		if e.IsDir() {
			metaPath := filepath.Join(baseDir, e.Name(), "meta.json")
			if _, err := os.Stat(metaPath); err == nil {
				var meta EpicAccountData
				d, _ := os.ReadFile(metaPath)
				json.Unmarshal(d, &meta)

				accounts = append(accounts, models.Account{
					ID:          "epic_" + meta.Name,
					DisplayName: meta.Name,
					Username:    meta.Name,
					Platform:    "Epic",
				})
			}
		}
	}

	return accounts
}

func copyFile(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()

	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	defer out.Close()

	_, err = io.Copy(out, in)
	return err
}
