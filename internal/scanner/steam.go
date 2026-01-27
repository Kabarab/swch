package scanner

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strconv"
	"strings"
	"swch/internal/models"
	"swch/internal/sys"
	"sync"
	"time"

	"github.com/andygrunwald/vdf"
)

// Глобальный кэш
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
	contentBytes, err := os.ReadFile(loginUsersPath)
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
		return os.WriteFile(loginUsersPath, []byte(content), 0644)
	}
	return nil
}

type steamApp struct {
	AppID int    `json:"appid"`
	Name  string `json:"name"`
}

type steamAppListResponse struct {
	Applist struct {
		Apps []steamApp `json:"apps"`
	} `json:"applist"`
}

type steamAppListFlat struct {
	Apps []steamApp `json:"apps"`
}

func getCacheFilePaths() []string {
	var paths []string
	cwd, _ := os.Getwd()
	paths = append(paths, filepath.Join(cwd, cacheFileName))
	configDir, _ := os.UserConfigDir()
	appDataPath := filepath.Join(configDir, "swch", cacheFileName)
	paths = append(paths, appDataPath)
	return paths
}

func loadCacheFromFile() {
	paths := getCacheFilePaths()
	for _, path := range paths {
		if _, err := os.Stat(path); err == nil {
			data, err := os.ReadFile(path)
			if err == nil {
				var loadedMap map[string]string
				if json.Unmarshal(data, &loadedMap) == nil && len(loadedMap) > 0 {
					cacheMutex.Lock()
					appNameCache = loadedMap
					cacheLoaded = true
					cacheMutex.Unlock()
					fmt.Printf("[Steam] Loaded %d game names from cache: %s\n", len(loadedMap), path)
					return
				}
			}
		}
	}
}

func saveCacheToFile() {
	cacheMutex.RLock()
	defer cacheMutex.RUnlock()
	if len(appNameCache) == 0 {
		return
	}
	data, err := json.Marshal(appNameCache)
	if err == nil {
		configDir, _ := os.UserConfigDir()
		appDir := filepath.Join(configDir, "swch")
		_ = os.MkdirAll(appDir, 0755)
		path := filepath.Join(appDir, cacheFileName)
		if err := os.WriteFile(path, data, 0644); err == nil {
			fmt.Printf("[Steam] Cache saved to %s\n", path)
		}
	}
}

// Создает минимальный список популярных игр, если интернет не работает
func createFallbackCache() {
	fmt.Println("[Steam] Network failed. Generating fallback game list...")
	fallback := map[string]string{
		"730":     "Counter-Strike 2",
		"570":     "Dota 2",
		"440":     "Team Fortress 2",
		"578080":  "PUBG: BATTLEGROUNDS",
		"271590":  "Grand Theft Auto V",
		"1172470": "Apex Legends",
		"105600":  "Terraria",
		"252490":  "Rust",
		"292030":  "The Witcher 3: Wild Hunt",
		"1085660": "Destiny 2",
	}

	cacheMutex.Lock()
	appNameCache = fallback
	cacheLoaded = true
	cacheMutex.Unlock()
	saveCacheToFile()
}

func ensureGameNamesLoaded() {
	if cacheLoaded {
		return
	}

	loadCacheFromFile()
	if cacheLoaded {
		return
	}

	cacheMutex.Lock()
	appNameCache = make(map[string]string)
	cacheMutex.Unlock()

	urls := []string{
		"https://api.steampowered.com/ISteamApps/GetAppList/v0002/?format=json",
		"https://raw.githubusercontent.com/oxypanel/Steam-App-List/main/data/apps.json",
		"https://raw.githubusercontent.com/teslaworks/steam-app-list/master/steam_app_list.json",
		"https://raw.githubusercontent.com/WindowsGSM/SteamAppInfo/master/apps.json",
	}

	client := &http.Client{Timeout: 10 * time.Second}
	var success bool

	fmt.Println("[Steam] Downloading App List...")

	for _, u := range urls {
		req, _ := http.NewRequest("GET", u, nil)
		req.Header.Set("User-Agent", "Valve/Steam HTTP Client 1.0")

		resp, err := client.Do(req)
		if err != nil {
			fmt.Printf(" - Error fetching %s: %v\n", u, err)
			continue
		}
		defer resp.Body.Close()

		if resp.StatusCode != 200 {
			fmt.Printf(" - HTTP Error %d for %s\n", resp.StatusCode, u)
			continue
		}

		bodyBytes, err := io.ReadAll(resp.Body)
		if err != nil {
			continue
		}

		if len(bodyBytes) > 0 && bodyBytes[0] == '<' {
			continue
		}

		var apps []steamApp
		var resultStandard steamAppListResponse
		if err := json.Unmarshal(bodyBytes, &resultStandard); err == nil && len(resultStandard.Applist.Apps) > 0 {
			apps = resultStandard.Applist.Apps
		} else {
			var resultFlat steamAppListFlat
			if err := json.Unmarshal(bodyBytes, &resultFlat); err == nil && len(resultFlat.Apps) > 0 {
				apps = resultFlat.Apps
			} else {
				var resultArray []steamApp
				if err := json.Unmarshal(bodyBytes, &resultArray); err == nil && len(resultArray) > 0 {
					apps = resultArray
				}
			}
		}

		if len(apps) > 0 {
			cacheMutex.Lock()
			for _, app := range apps {
				if app.Name != "" {
					appNameCache[strconv.Itoa(app.AppID)] = app.Name
				}
			}
			cacheLoaded = true
			cacheMutex.Unlock()
			success = true
			fmt.Printf("[Steam] Successfully downloaded %d game names.\n", len(apps))
			saveCacheToFile()
			break
		}
	}

	if !success {
		// ВАЖНО: Если ничего не скачалось, создаем фейковый кэш, чтобы программа работала
		createFallbackCache()
	}
}

func (s *SteamScanner) GetGames() []models.LibraryGame {
	var games []models.LibraryGame
	if s.Path == "" {
		return games
	}

	ensureGameNamesLoaded()

	accounts := s.GetAccounts()
	libraryPaths := s.getLibraryFolders()
	installedAppIDs := make(map[string]bool)

	// --- 1. Установленные игры ---
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

				// Логика поддержки Mac для установленных игр
				isMacSupported := false
				if runtime.GOOS == "darwin" {
					// Если мы на маке и игра установлена в Steam, значит она точно работает
					isMacSupported = true
				} else {
					// Если мы на Windows, проверяем через API (или считаем false, если не хотим ждать)
					// isMacSupported = checkMacSupport(appID)
				}

				games = append(games, models.LibraryGame{
					ID:                  appID,
					Name:                name,
					Platform:            "Steam",
					IconURL:             fmt.Sprintf("https://cdn.cloudflare.steamstatic.com/steam/apps/%s/header.jpg", appID),
					ExePath:             fullPath,
					AvailableOnAccounts: owners,
					IsInstalled:         true,
					IsMacSupported:      isMacSupported, // <-- Новое поле
				})
				installedAppIDs[appID] = true
			}
		}
	}

	// --- 2. Неустановленные игры ---
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
			// Попытка найти список приложений в разных структурах VDF
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
				// Пропускаем уже добавленные (установленные)
				if _, exists := installedAppIDs[appID]; exists {
					continue
				}
				if _, err := strconv.Atoi(appID); err != nil {
					continue
				}

				gameName := ""
				if details, ok := appData.(map[string]interface{}); ok {
					if n, ok := details["name"].(string); ok && n != "" {
						gameName = n
					} else if common, ok := details["common"].(map[string]interface{}); ok {
						if cn, ok := common["name"].(string); ok && cn != "" {
							gameName = cn
						}
					}
				}

				// Если имени нет в конфиге, берем из кэша
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
					// Игра уже есть в списке (от другого аккаунта), добавляем владельца
					games[foundIdx].AvailableOnAccounts = append(games[foundIdx].AvailableOnAccounts, ownerStat)
					if (games[foundIdx].Name == "" || strings.HasPrefix(games[foundIdx].Name, "Steam App")) && gameName != "" {
						games[foundIdx].Name = gameName
					}
				} else {
					if gameName == "" {
						gameName = fmt.Sprintf("Steam App %s", appID)
					}

					// Логика поддержки Mac для неустановленных игр
					isMacSupported := false

					// ВНИМАНИЕ: Если включить этот блок, загрузка библиотеки замедлится!
					// Steam API имеет лимиты (около 200 запросов в 5 минут).
					// Рекомендуется вызывать checkMacSupport только по требованию пользователя.
					/*
						if runtime.GOOS == "darwin" {
							isMacSupported = checkMacSupport(appID)
						}
					*/

					games = append(games, models.LibraryGame{
						ID:                  appID,
						Name:                gameName,
						Platform:            "Steam",
						IconURL:             fmt.Sprintf("https://cdn.cloudflare.steamstatic.com/steam/apps/%s/header.jpg", appID),
						ExePath:             "",
						AvailableOnAccounts: []models.AccountStat{ownerStat},
						IsInstalled:         false,
						IsMacSupported:      isMacSupported, // <-- Новое поле
					})
				}
			}
		}
	}

	return games
}

// Структуры для ответа Steam Store API
type storeAppDetails struct {
	Success bool `json:"success"`
	Data    struct {
		Platforms struct {
			Windows bool `json:"windows"`
			Mac     bool `json:"mac"`
			Linux   bool `json:"linux"`
		} `json:"platforms"`
	} `json:"data"`
}

// Кэш совместимости (чтобы не спамить API при каждом запуске)
var osSupportCache = make(map[string]bool)

func checkMacSupport(appID string) bool {
	// 1. Если уже проверяли - возвращаем из памяти
	if val, ok := osSupportCache[appID]; ok {
		return val
	}

	// 2. Делаем запрос к API магазина
	url := fmt.Sprintf("https://store.steampowered.com/api/appdetails?appids=%s&filters=platforms", appID)
	resp, err := http.Get(url)
	if err != nil {
		return false // Если ошибка сети, считаем что не поддерживается (безопасно)
	}
	defer resp.Body.Close()

	var result map[string]storeAppDetails
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return false
	}

	// 3. Парсим ответ
	if appData, ok := result[appID]; ok && appData.Success {
		isMac := appData.Data.Platforms.Mac
		osSupportCache[appID] = isMac // Сохраняем в кэш
		return isMac
	}

	return false
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
	// 1. Проверяем localconfig.vdf (старый метод)
	localConfigPath := filepath.Join(s.Path, "userdata", steamID3, "config", "localconfig.vdf")
	if fileContainsAppID(localConfigPath, appID) {
		return true
	}

	// 2. Проверяем sharedconfig.vdf (новый надежный метод)
	// Путь: userdata/<id>/7/remote/sharedconfig.vdf
	sharedConfigPath := filepath.Join(s.Path, "userdata", steamID3, "7", "remote", "sharedconfig.vdf")
	if fileContainsAppID(sharedConfigPath, appID) {
		return true
	}

	return false
}
func fileContainsAppID(path, appID string) bool {
	contentBytes, err := os.ReadFile(path)
	if err != nil {
		return false
	}
	// Ищем точное совпадение "appid", чтобы не найти "12" внутри "12345"
	// Обычно в VDF это выглядит как key "123" или value "123"
	searchStr := fmt.Sprintf(`"%s"`, appID)
	return strings.Contains(string(contentBytes), searchStr)
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
