package scanner

import (
	"encoding/json"
	"fmt"
	"io/ioutil"
	"net/http"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"swch/internal/models"
	"swch/internal/sys"
	"sync"
	"time"

	"github.com/andygrunwald/vdf"
)

// Глобальный кэш и мьютекс для безопасности
var (
	appNameCache map[string]string
	cacheMutex   sync.RWMutex
	cacheLoaded  bool
)

const cacheFileName = "steam_cache.json"

type SteamScanner struct {
	Path string
}

func NewSteamScanner() *SteamScanner {
	path, _ := sys.GetSteamPath()
	return &SteamScanner{Path: path}
}

// SetUserActive обновляет loginusers.vdf
func (s *SteamScanner) SetUserActive(targetUsername string) error {
	loginUsersPath := filepath.Join(s.Path, "config", "loginusers.vdf")
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

// Структура ответа Steam API
type steamAppListResponse struct {
	Applist struct {
		Apps []struct {
			AppID int    `json:"appid"`
			Name  string `json:"name"`
		} `json:"apps"`
	} `json:"applist"`
}

// Загрузка кэша из локального файла
func loadCacheFromFile() {
	if _, err := os.Stat(cacheFileName); err == nil {
		data, err := ioutil.ReadFile(cacheFileName)
		if err == nil {
			var loadedMap map[string]string
			if json.Unmarshal(data, &loadedMap) == nil && len(loadedMap) > 0 {
				cacheMutex.Lock()
				appNameCache = loadedMap
				cacheLoaded = true
				cacheMutex.Unlock()
				fmt.Printf("Loaded %d game names from local cache file.\n", len(loadedMap))
			}
		}
	}
}

// Сохранение кэша в файл
func saveCacheToFile() {
	cacheMutex.RLock()
	defer cacheMutex.RUnlock()
	if len(appNameCache) == 0 {
		return
	}
	data, err := json.Marshal(appNameCache)
	if err == nil {
		_ = ioutil.WriteFile(cacheFileName, data, 0644)
		fmt.Println("Game names cached to disk successfully.")
	}
}

// Функция инициализации названий игр
func ensureGameNamesLoaded() {
	if cacheLoaded {
		return
	}

	// 1. Пробуем загрузить с диска
	loadCacheFromFile()
	if cacheLoaded {
		// Если файл есть и он старый (старше 7 дней), можно попробовать обновить в фоне,
		// но пока просто вернем то, что есть.
		return
	}

	cacheMutex.Lock()
	appNameCache = make(map[string]string)
	cacheMutex.Unlock()

	// 2. Если файла нет, качаем из интернета
	urls := []string{
		"https://cdn.jsdelivr.net/gh/SteamDatabase/SteamAppList@master/apps.json",       // Самый надежный CDN
		"https://api.steampowered.com/ISteamApps/GetAppList/v2/",                        // Официальный API
		"https://raw.githubusercontent.com/SteamDatabase/SteamAppList/master/apps.json", // Оригинал
	}

	client := &http.Client{Timeout: 15 * time.Second}
	var success bool

	fmt.Println("Downloading Steam App List...")

	for _, u := range urls {
		req, _ := http.NewRequest("GET", u, nil)
		req.Header.Set("User-Agent", "Valve/Steam HTTP Client 1.0") // Притворяемся клиентом Steam

		resp, err := client.Do(req)
		if err != nil {
			continue
		}
		defer resp.Body.Close()

		// Читаем начало тела, чтобы проверить, не HTML ли это
		bodyBytes, err := ioutil.ReadAll(resp.Body)
		if err != nil {
			continue
		}

		// Если ответ пустой или начинается с < (HTML ошибка), пропускаем
		if len(bodyBytes) == 0 || bodyBytes[0] == '<' {
			continue
		}

		var result steamAppListResponse
		if err := json.Unmarshal(bodyBytes, &result); err == nil && len(result.Applist.Apps) > 0 {
			cacheMutex.Lock()
			for _, app := range result.Applist.Apps {
				appNameCache[strconv.Itoa(app.AppID)] = app.Name
			}
			cacheLoaded = true
			cacheMutex.Unlock()
			success = true
			fmt.Printf("Successfully downloaded %d game names from %s\n", len(result.Applist.Apps), u)

			// Сохраняем на диск для следующего раза
			saveCacheToFile()
			break
		}
	}

	if !success {
		fmt.Println("Warning: Could not fetch game list from web. Uninstalled games will show IDs.")
	}
}

// GetGames возвращает игры
func (s *SteamScanner) GetGames() []models.LibraryGame {
	var games []models.LibraryGame
	if s.Path == "" {
		return games
	}

	// Запускаем загрузку имен (если еще не загружены)
	ensureGameNamesLoaded()

	accounts := s.GetAccounts()
	libraryPaths := s.getLibraryFolders()
	installedAppIDs := make(map[string]bool)

	// 1. Установленные игры
	for _, libPath := range libraryPaths {
		steamAppsPath := filepath.Join(libPath, "steamapps")
		files, err := os.ReadDir(steamAppsPath)
		if err != nil {
			continue
		}

		for _, f := range files {
			if strings.HasPrefix(f.Name(), "appmanifest_") && strings.HasSuffix(f.Name(), ".acf") {
				data := parseVdf(filepath.Join(steamAppsPath, f.Name()))
				if data == nil {
					continue
				}
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

	// 2. Неустановленные игры
	for _, acc := range accounts {
		configFiles := []string{
			filepath.Join(s.Path, "userdata", acc.ID, "config", "localconfig.vdf"),
			filepath.Join(s.Path, "userdata", acc.ID, "7", "remote", "sharedconfig.vdf"),
		}

		for _, path := range configFiles {
			data := parseVdf(path)
			if data == nil {
				continue
			}

			var apps map[string]interface{}
			if store, ok := data["UserLocalConfigStore"].(map[string]interface{}); ok {
				if soft, ok := store["Software"].(map[string]interface{}); ok {
					if valve, ok := soft["Valve"].(map[string]interface{}); ok {
						if steam, ok := valve["Steam"].(map[string]interface{}); ok {
							apps, _ = steam["apps"].(map[string]interface{})
						}
					}
				}
			} else if root, ok := data["UserRoamableConfigStore"].(map[string]interface{}); ok {
				if soft, ok := root["Software"].(map[string]interface{}); ok {
					if valve, ok := soft["Valve"].(map[string]interface{}); ok {
						if steam, ok := valve["Steam"].(map[string]interface{}); ok {
							apps, _ = steam["Apps"].(map[string]interface{})
						}
					}
				}
			}

			if apps == nil {
				continue
			}

			for appID, appData := range apps {
				if _, exists := installedAppIDs[appID]; exists {
					continue
				}
				if _, err := strconv.Atoi(appID); err != nil {
					continue
				}

				gameName := ""
				// Пытаемся найти имя в VDF (иногда оно там есть)
				if details, ok := appData.(map[string]interface{}); ok {
					if n, ok := details["name"].(string); ok && n != "" {
						gameName = n
					} else if common, ok := details["common"].(map[string]interface{}); ok {
						if cn, ok := common["name"].(string); ok && cn != "" {
							gameName = cn
						}
					}
				}

				// Если имени нет, берем из нашего Web-кэша
				if gameName == "" {
					cacheMutex.RLock()
					if name, found := appNameCache[appID]; found {
						gameName = name
					}
					cacheMutex.RUnlock()
				}

				ownerStat := models.AccountStat{
					AccountID:   acc.ID,
					DisplayName: acc.DisplayName,
					Username:    acc.Username,
				}

				foundIdx := -1
				for i, g := range games {
					if g.ID == appID {
						foundIdx = i
						break
					}
				}

				if foundIdx != -1 {
					games[foundIdx].AvailableOnAccounts = append(games[foundIdx].AvailableOnAccounts, ownerStat)
					// Обновляем имя, если нашли лучшее
					isGenericName := games[foundIdx].Name == "" || strings.HasPrefix(games[foundIdx].Name, "Steam App")
					if isGenericName && gameName != "" {
						games[foundIdx].Name = gameName
					}
				} else {
					if gameName == "" {
						gameName = fmt.Sprintf("Steam App %s", appID)
					}
					games = append(games, models.LibraryGame{
						ID:                  appID,
						Name:                gameName,
						Platform:            "Steam",
						IconURL:             fmt.Sprintf("https://cdn.cloudflare.steamstatic.com/steam/apps/%s/header.jpg", appID),
						ExePath:             "",
						AvailableOnAccounts: []models.AccountStat{ownerStat},
						IsInstalled:         false,
					})
				}
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
		if loginData != nil {
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
