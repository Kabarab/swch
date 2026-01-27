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

type RiotAccountData struct {
	Name string `json:"name"`
}

func getRiotConfigDir() string {
	configDir, _ := os.UserConfigDir()
	path := filepath.Join(configDir, "swch", "riot_accounts")
	_ = os.MkdirAll(path, 0755)
	return path
}

// ScanRiotGames пока оставляем специфичным для Windows (ProgramData),
// так как точная структура установки игр на macOS может отличаться.
// Но аккаунты будут работать.
func ScanRiotGames() []models.LibraryGame {
	var games []models.LibraryGame
	programData := os.Getenv("ProgramData")
	if programData == "" {
		// На macOS игр в ProgramData нет, возвращаем пустой список или
		// можно добавить логику поиска в /Applications
		return games
	}

	jsonPath := filepath.Join(programData, "Riot Games", "RiotClientInstalls.json")
	data, err := os.ReadFile(jsonPath)
	if err != nil {
		return games
	}

	var installs map[string]string
	if err := json.Unmarshal(data, &installs); err != nil {
		return games
	}

	for key, path := range installs {
		if key == "rc_default" || key == "rc_live" {
			continue
		}

		gameID := strings.TrimSuffix(key, "_live")
		name := gameID
		iconURL := ""

		switch gameID {
		case "valorant":
			name = "VALORANT"
			iconURL = "https://img.icons8.com/color/48/valorant.png"
		case "league_of_legends":
			name = "League of Legends"
			iconURL = "https://img.icons8.com/color/48/league-of-legends.png"
		case "bacon":
			name = "Legends of Runeterra"
			iconURL = "https://img.icons8.com/fluency/48/legends-of-runeterra.png"
		case "2xko":
			name = "2XKO"
		}

		games = append(games, models.LibraryGame{
			ID:                  gameID,
			Name:                name,
			Platform:            "Riot",
			IconURL:             iconURL,
			ExePath:             path,
			AvailableOnAccounts: []models.AccountStat{},
			IsInstalled:         true,
		})
	}
	return games
}

func SaveCurrentRiotAccount(name string) error {
	if name == "" {
		return fmt.Errorf("name is empty")
	}

	// ИСПОЛЬЗУЕМ КРОССПЛАТФОРМЕННУЮ ФУНКЦИЮ ИЗ SYS
	srcPath := sys.GetRiotPrivateSettingsPath()

	if _, err := os.Stat(srcPath); os.IsNotExist(err) {
		return fmt.Errorf("Riot settings not found. Please login to Riot Client first.")
	}

	destDir := filepath.Join(getRiotConfigDir(), name)
	os.MkdirAll(destDir, 0755)

	if err := copyRiotFile(srcPath, filepath.Join(destDir, "RiotClientPrivateSettings.yaml")); err != nil {
		return err
	}

	meta := RiotAccountData{Name: name}
	data, _ := json.MarshalIndent(meta, "", "  ")
	return os.WriteFile(filepath.Join(destDir, "meta.json"), data, 0644)
}

func SwitchRiotAccount(name string) error {
	dir := filepath.Join(getRiotConfigDir(), name)
	yamlSource := filepath.Join(dir, "RiotClientPrivateSettings.yaml")

	if _, err := os.Stat(yamlSource); os.IsNotExist(err) {
		return fmt.Errorf("account backup not found")
	}

	sys.KillRiot()

	// ИСПОЛЬЗУЕМ КРОССПЛАТФОРМЕННУЮ ФУНКЦИЮ ИЗ SYS
	targetPath := sys.GetRiotPrivateSettingsPath()
	os.Remove(targetPath)

	if err := copyRiotFile(yamlSource, targetPath); err != nil {
		return fmt.Errorf("failed to copy settings: %v", err)
	}

	return nil
}

func ScanRiotAccounts() []models.Account {
	var accounts []models.Account
	baseDir := getRiotConfigDir()

	entries, err := os.ReadDir(baseDir)
	if err != nil {
		return accounts
	}

	for _, e := range entries {
		if e.IsDir() {
			metaPath := filepath.Join(baseDir, e.Name(), "meta.json")
			if _, err := os.Stat(metaPath); err == nil {
				var meta RiotAccountData
				d, _ := os.ReadFile(metaPath)
				json.Unmarshal(d, &meta)

				accounts = append(accounts, models.Account{
					ID:          "riot_" + meta.Name,
					DisplayName: meta.Name,
					Username:    meta.Name,
					Platform:    "Riot",
				})
			}
		}
	}
	return accounts
}

func copyRiotFile(src, dst string) error {
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
