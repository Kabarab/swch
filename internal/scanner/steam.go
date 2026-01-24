package scanner

import (
	"fmt"
	"io/ioutil"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"swch/internal/models"
	"swch/internal/sys"
	"time"

	"github.com/andygrunwald/vdf"
)

type SteamScanner struct {
	Path string
}

func NewSteamScanner() *SteamScanner {
	path, _ := sys.GetSteamPath()
	return &SteamScanner{Path: path}
}

// SetUserActive обновляет loginusers.vdf, делая пользователя активным
func (s *SteamScanner) SetUserActive(targetUsername string) error {
	loginUsersPath := filepath.Join(s.Path, "config", "loginusers.vdf")
	// (код функции остался без изменений, сокращено для краткости, он корректен)
	contentBytes, err := ioutil.ReadFile(loginUsersPath)
	if err != nil {
		return err
	}
	content := string(contentBytes)

	reReset := regexp.MustCompile(`"MostRecent"\s+"1"`)
	content = reReset.ReplaceAllString(content, `"MostRecent"       "0"`)

	targetPattern := fmt.Sprintf(`"(?i)%s"`, regexp.QuoteMeta(targetUsername))
	loc := regexp.MustCompile(targetPattern).FindStringIndex(content)

	if loc != nil {
		restOfFile := content[loc[1]:]
		blockEnd := strings.Index(restOfFile, "}")
		if blockEnd == -1 {
			blockEnd = len(restOfFile)
		}
		userBlock := restOfFile[:blockEnd]

		if strings.Contains(userBlock, `"MostRecent"`) {
			userBlock = regexp.MustCompile(`"MostRecent"\s+"0"`).ReplaceAllString(userBlock, `"MostRecent"      "1"`)
		} else {
			userBlock += "\n\t\t\"MostRecent\"      \"1\""
		}

		currentTimestamp := fmt.Sprintf(`"Timestamp"        "%d"`, time.Now().Unix())
		reTimestamp := regexp.MustCompile(`"Timestamp"\s+"\d+"`)
		if reTimestamp.MatchString(userBlock) {
			userBlock = reTimestamp.ReplaceAllString(userBlock, currentTimestamp)
		} else {
			userBlock += "\n\t\t" + currentTimestamp
		}

		reAutoLogin := regexp.MustCompile(`"AllowAutoLogin"\s+"\d+"`)
		if reAutoLogin.MatchString(userBlock) {
			userBlock = reAutoLogin.ReplaceAllString(userBlock, `"AllowAutoLogin"       "1"`)
		} else {
			userBlock += "\n\t\t\"AllowAutoLogin\"      \"1\""
		}
		content = content[:loc[1]] + userBlock + content[loc[1]+blockEnd:]
		return ioutil.WriteFile(loginUsersPath, []byte(content), 0644)
	}
	return nil
}

// GetGames возвращает ВСЕ игры: и установленные, и просто купленные.
func (s *SteamScanner) GetGames() []models.LibraryGame {
	var games []models.LibraryGame
	if s.Path == "" {
		return games
	}

	accounts := s.GetAccounts()
	libraryPaths := s.getLibraryFolders()

	// Map для отслеживания уже добавленных (установленных) игр
	installedAppIDs := make(map[string]bool)

	// 1. Сканируем УСТАНОВЛЕННЫЕ игры (appmanifest_*.acf)
	for _, libPath := range libraryPaths {
		steamAppsPath := filepath.Join(libPath, "steamapps")
		files, err := os.ReadDir(steamAppsPath)
		if err != nil {
			continue
		}

		for _, f := range files {
			if strings.HasPrefix(f.Name(), "appmanifest_") && strings.HasSuffix(f.Name(), ".acf") {
				data := parseVdf(filepath.Join(steamAppsPath, f.Name()))
				appState, ok := data["AppState"].(map[string]interface{})
				if !ok {
					continue
				}

				name, _ := appState["name"].(string)
				appID, _ := appState["appid"].(string)
				installDir, _ := appState["installdir"].(string)
				fullPath := filepath.Join(steamAppsPath, "common", installDir)

				var owners []models.AccountStat
				for _, acc := range accounts {
					if s.ownsGame(acc.ID, appID) {
						owners = append(owners, models.AccountStat{
							AccountID:   acc.ID,
							DisplayName: acc.DisplayName,
							Username:    acc.Username,
							PlaytimeMin: 0,
						})
					}
				}

				games = append(games, models.LibraryGame{
					ID:                  appID,
					Name:                name,
					Platform:            "Steam",
					IconURL:             fmt.Sprintf("https://cdn.cloudflare.steamstatic.com/steam/apps/%s/header.jpg", appID),
					ExePath:             fullPath,
					AvailableOnAccounts: owners,
					IsInstalled:         true,
				})
				installedAppIDs[appID] = true
			}
		}
	}

	// 2. Сканируем НЕУСТАНОВЛЕННЫЕ игры из localconfig.vdf
	// Мы пройдемся по каждому пользователю и соберем их игры
	for _, acc := range accounts {
		localConfigPath := filepath.Join(s.Path, "userdata", acc.ID, "config", "localconfig.vdf")
		data := parseVdf(localConfigPath)
		if data == nil {
			continue
		}

		// Путь в VDF: UserLocalConfigStore -> Software -> Valve -> Steam -> apps
		// Иногда корень может отличаться, пробуем навигацию
		store, ok := data["UserLocalConfigStore"].(map[string]interface{})
		if !ok {
			continue
		}
		software, ok := store["Software"].(map[string]interface{})
		if !ok {
			continue
		}
		valve, ok := software["Valve"].(map[string]interface{})
		if !ok {
			continue
		}
		steam, ok := valve["Steam"].(map[string]interface{})
		if !ok {
			continue
		}
		apps, ok := steam["apps"].(map[string]interface{})
		if !ok {
			continue
		}

		for appID, _ := range apps {
			// Пропускаем уже найденные (установленные) игры
			if _, exists := installedAppIDs[appID]; exists {
				continue
			}
			// Пропускаем системные id
			if _, err := strconv.Atoi(appID); err != nil {
				continue
			}

			// Добавляем игру. Имя берем generic, т.к. localconfig не хранит красивых имен.
			// Картинка подтянется по ID.
			gameName := fmt.Sprintf("Steam App %s", appID)

			// Проверяем, не добавили ли мы эту игру уже в этом цикле (от другого аккаунта)
			foundIdx := -1
			for i, g := range games {
				if g.ID == appID {
					foundIdx = i
					break
				}
			}

			ownerStat := models.AccountStat{
				AccountID:   acc.ID,
				DisplayName: acc.DisplayName,
				Username:    acc.Username,
			}

			if foundIdx != -1 {
				// Игра уже в списке, добавляем владельца
				games[foundIdx].AvailableOnAccounts = append(games[foundIdx].AvailableOnAccounts, ownerStat)
			} else {
				// Новая неустановленная игра
				games = append(games, models.LibraryGame{
					ID:                  appID,
					Name:                gameName, // Фронтенд может попробовать переименовать по ID если нужно
					Platform:            "Steam",
					IconURL:             fmt.Sprintf("https://cdn.cloudflare.steamstatic.com/steam/apps/%s/header.jpg", appID),
					ExePath:             "",
					AvailableOnAccounts: []models.AccountStat{ownerStat},
					IsInstalled:         false,
				})
			}
		}
	}

	return games
}

func (s *SteamScanner) getLibraryFolders() []string {
	paths := []string{s.Path}
	vdfPath := filepath.Join(s.Path, "steamapps", "libraryfolders.vdf")
	f, err := os.Open(vdfPath)
	if err != nil {
		return paths
	}
	defer f.Close()

	p := vdf.NewParser(f)
	m, _ := p.Parse()
	if m != nil {
		if libFolders, ok := m["libraryfolders"].(map[string]interface{}); ok {
			for _, v := range libFolders {
				if folderData, ok := v.(map[string]interface{}); ok {
					if path, ok := folderData["path"].(string); ok {
						if !strings.EqualFold(path, s.Path) {
							paths = append(paths, path)
						}
					}
				}
			}
		}
	}
	return paths
}

func (s *SteamScanner) GetAccounts() []models.Account {
	var accounts []models.Account
	loginUsersPath := filepath.Join(s.Path, "config", "loginusers.vdf")
	loginData := parseVdf(loginUsersPath)

	userDataPath := filepath.Join(s.Path, "userdata")
	entries, _ := os.ReadDir(userDataPath)

	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		steamID3 := entry.Name()
		if _, err := strconv.Atoi(steamID3); err != nil {
			continue
		}

		displayName := "User " + steamID3
		username := ""

		id3, _ := strconv.ParseInt(steamID3, 10, 64)
		id64 := id3 + 76561197960265728
		id64Str := strconv.FormatInt(id64, 10)

		var userData map[string]interface{}

		if users, ok := loginData["users"].(map[string]interface{}); ok {
			if u, found := users[id64Str].(map[string]interface{}); found {
				userData = u
			}
		}
		if userData == nil {
			if u, found := loginData[id64Str].(map[string]interface{}); found {
				userData = u
			}
		}

		if userData != nil {
			if n, ok := userData["PersonaName"].(string); ok {
				displayName = n
			}
			if a, ok := userData["AccountName"].(string); ok {
				username = a
			}
		}
		if username == "" {
			username = "UNKNOWN"
		}
		accounts = append(accounts, models.Account{
			ID:          steamID3,
			DisplayName: displayName,
			Username:    username,
			Platform:    "Steam",
		})
	}
	return accounts
}

func (s *SteamScanner) ownsGame(steamID3 string, appID string) bool {
	localConfigPath := filepath.Join(s.Path, "userdata", steamID3, "config", "localconfig.vdf")
	contentBytes, err := ioutil.ReadFile(localConfigPath)
	if err != nil {
		return false
	}
	// Простая проверка наличия ID в файле конфига
	return strings.Contains(string(contentBytes), fmt.Sprintf(`"%s"`, appID))
}

func parseVdf(path string) map[string]interface{} {
	f, err := os.Open(path)
	if err != nil {
		return nil
	}
	defer f.Close()
	p := vdf.NewParser(f)
	m, _ := p.Parse()
	return m
}
