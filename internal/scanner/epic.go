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

// EpicManifest структура файла .item (манифест игры)
type EpicManifest struct {
	FormatVersion   int    `json:"FormatVersion"`
	AppName         string `json:"AppName"`
	DisplayName     string `json:"DisplayName"`
	InstallLocation string `json:"InstallLocation"`
	MainGameAppName string `json:"MainGameAppName"`
}

// EpicAccountData хранит метаданные сохраненного аккаунта
type EpicAccountData struct {
	Name string `json:"name"`
	// RegistryId удален, так как для метода подмены папки Data он не критичен
}

// getEpicConfigDir возвращает путь, где мы храним бэкапы аккаунтов
func getEpicConfigDir() string {
	configDir, _ := os.UserConfigDir()
	path := filepath.Join(configDir, "swch", "epic_accounts")
	_ = os.MkdirAll(path, 0755)
	return path
}

// getEpicAuthDataPath возвращает путь к папке Data (где Epic хранит токены сессии)
func getEpicAuthDataPath() string {
	return sys.GetEpicAuthDataDir()
}

// SaveCurrentEpicAccount сохраняет текущую сессию Epic (папку Data)
func SaveCurrentEpicAccount(name string) error {
	if name == "" {
		return fmt.Errorf("name is empty")
	}

	// 1. Проверяем наличие папки Data (значит пользователь логинился)
	srcDataPath := getEpicAuthDataPath()
	if _, err := os.Stat(srcDataPath); os.IsNotExist(err) {
		return fmt.Errorf("Epic Data folder not found. Please login to Epic Games Launcher first.")
	}

	// Создаем папку для хранения этого аккаунта
	destDir := filepath.Join(getEpicConfigDir(), name)
	if err := os.MkdirAll(destDir, 0755); err != nil {
		return err
	}

	// 2. Копируем папку Data (токены)
	destDataPath := filepath.Join(destDir, "Data")
	os.RemoveAll(destDataPath) // Удаляем старый бэкап если был

	if err := copyDir(srcDataPath, destDataPath); err != nil {
		return fmt.Errorf("failed to copy auth data: %v", err)
	}

	// 3. Сохраняем метаданные (имя аккаунта)
	meta := EpicAccountData{
		Name: name,
	}
	data, _ := json.MarshalIndent(meta, "", "  ")
	return os.WriteFile(filepath.Join(destDir, "meta.json"), data, 0644)
}

// SwitchEpicAccount переключает аккаунт путем подмены папки Data
func SwitchEpicAccount(name string) error {
	// Путь к сохраненному аккаунту
	storedAccountDir := filepath.Join(getEpicConfigDir(), name)
	metaPath := filepath.Join(storedAccountDir, "meta.json")

	if _, err := os.Stat(metaPath); os.IsNotExist(err) {
		return fmt.Errorf("account not found")
	}

	// 1. Важно: Убиваем процесс Epic Games, иначе файлы будут заняты
	if err := sys.KillEpic(); err != nil {
		// Логируем ошибку, но пробуем продолжить (вдруг процесс уже мертв)
		fmt.Printf("Warning killing epic: %v\n", err)
	}

	// 2. Очищаем текущую папку Data в системе
	realDataPath := getEpicAuthDataPath()
	if err := os.RemoveAll(realDataPath); err != nil {
		return fmt.Errorf("failed to remove current session files: %v", err)
	}

	// 3. Восстанавливаем папку Data из бэкапа
	storedDataPath := filepath.Join(storedAccountDir, "Data")
	if err := copyDir(storedDataPath, realDataPath); err != nil {
		return fmt.Errorf("failed to restore session files: %v", err)
	}

	// Готово. При следующем запуске Epic подхватит эти файлы.
	return nil
}

// ScanEpicGames сканирует установленные игры
func ScanEpicGames() []models.LibraryGame {
	var games []models.LibraryGame

	// Используем кроссплатформенный путь из пакета sys
	manifestPath := sys.GetEpicManifestsDir()

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

// ScanEpicAccounts сканирует папку swch на наличие сохраненных аккаунтов
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

// Вспомогательные функции

// copyDir рекурсивно копирует директорию
func copyDir(src string, dst string) error {
	srcInfo, err := os.Stat(src)
	if err != nil {
		return err
	}

	if err := os.MkdirAll(dst, srcInfo.Mode()); err != nil {
		return err
	}

	entries, err := os.ReadDir(src)
	if err != nil {
		return err
	}

	for _, entry := range entries {
		srcPath := filepath.Join(src, entry.Name())
		dstPath := filepath.Join(dst, entry.Name())

		if entry.IsDir() {
			if err := copyDir(srcPath, dstPath); err != nil {
				return err
			}
		} else {
			if err := copyFile(srcPath, dstPath); err != nil {
				return err
			}
		}
	}
	return nil
}

// copyFile копирует один файл
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
