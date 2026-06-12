package scanner

import (
	"encoding/json"
	"encoding/xml"
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
	// StorageOverride подменяет каталог хранения снапшотов сессий (для тестов).
	StorageOverride string
}

func NewSteamScanner() *SteamScanner {
	path, _ := sys.GetSteamPath()
	return &SteamScanner{Path: path}
}

// --- XML структуры (логика из steam-app-id-finder) ---
type steamGamesListXML struct {
	XMLName xml.Name       `xml:"gamesList"`
	Games   steamGamesData `xml:"games"`
	Error   string         `xml:"error"`
}

type steamGamesData struct {
	Game []steamGameXML `xml:"game"`
}

type steamGameXML struct {
	AppID string `xml:"appID"`
	Name  string `xml:"name"`
}

// -----------------------------------------------------

// --- Store API структуры ---
type storeAppDetailsResponse struct {
	Success bool `json:"success"`
	Data    struct {
		Name      string `json:"name"`
		Platforms struct {
			Windows bool `json:"windows"`
			Mac     bool `json:"mac"`
			Linux   bool `json:"linux"`
		} `json:"platforms"`
	} `json:"data"`
}

// ---------------------------

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

	targetPattern := fmt.Sprintf(`(?i)"%s"`, regexp.QuoteMeta(targetUsername))
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
		os.WriteFile(path, data, 0644)
	}
}

func createFallbackCache() {
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
		"https://raw.githubusercontent.com/WindowsGSM/SteamAppInfo/master/apps.json",
	}

	client := &http.Client{Timeout: 5 * time.Second}
	var success bool

	for _, u := range urls {
		req, _ := http.NewRequest("GET", u, nil)
		req.Header.Set("User-Agent", "Valve/Steam HTTP Client 1.0")
		resp, err := client.Do(req)
		if err != nil {
			continue
		}
		defer resp.Body.Close()

		if resp.StatusCode != 200 {
			continue
		}

		bodyBytes, err := io.ReadAll(resp.Body)
		if err != nil || len(bodyBytes) == 0 || bodyBytes[0] == '<' {
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
			saveCacheToFile()
			break
		}
	}

	if !success {
		createFallbackCache()
	}
}

// fetchOwnedGamesXML запрашивает список купленных игр аккаунта через
// Steam Community XML API (steamcommunity.com/profiles/<id64>/games?xml=1).
// Пустой результат — профиль скрыт, сеть недоступна или ID невалиден.
func (s *SteamScanner) fetchOwnedGamesXML(acc models.Account) []steamGameXML {
	id3, err := strconv.ParseInt(acc.ID, 10, 64)
	if err != nil {
		return nil
	}
	id64 := id3 + 76561197960265728
	url := fmt.Sprintf("https://steamcommunity.com/profiles/%d/games?xml=1", id64)

	req, _ := http.NewRequest("GET", url, nil)
	// User-Agent важен, иначе Steam блокирует
	req.Header.Set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")

	client := &http.Client{Timeout: 5 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		return nil
	}
	defer resp.Body.Close()

	if resp.StatusCode != 200 {
		return nil
	}

	var list steamGamesListXML
	if err := xml.NewDecoder(resp.Body).Decode(&list); err != nil {
		return nil
	}
	if list.Error != "" {
		return nil // Профиль скрыт или ошибка
	}
	return list.Games.Game
}

// fetchSingleGameName запрашивает имя игры через Store API
func fetchSingleGameName(appID string) string {
	url := fmt.Sprintf("https://store.steampowered.com/api/appdetails?appids=%s&filters=basic", appID)
	client := &http.Client{Timeout: 3 * time.Second}

	resp, err := client.Get(url)
	if err != nil {
		return ""
	}
	defer resp.Body.Close()

	var result map[string]storeAppDetailsResponse
	if err := json.NewDecoder(resp.Body).Decode(&result); err == nil {
		if data, ok := result[appID]; ok && data.Success {
			return data.Data.Name
		}
	}
	return ""
}

// ScanInstalledGames implements interfaces.LocalLibraryScanner:
// парсит appmanifest_*.acf во всех библиотеках steamapps (включая
// дополнительные диски из libraryfolders.vdf). Сеть не используется:
// имена берутся из манифестов, владельцы — из локальных userdata-конфигов.
func (s *SteamScanner) ScanInstalledGames() ([]models.LibraryGame, error) {
	var games []models.LibraryGame
	if s.Path == "" {
		return games, fmt.Errorf("steam path not found")
	}

	accounts := s.GetAccounts()

	for _, libPath := range s.getLibraryFolders() {
		steamAppsPath := filepath.Join(libPath, "steamapps")
		files, err := os.ReadDir(steamAppsPath)
		if err != nil {
			continue
		}

		for _, f := range files {
			if !strings.HasPrefix(f.Name(), "appmanifest_") || !strings.HasSuffix(f.Name(), ".acf") {
				continue
			}
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

			isMacSupported := false
			if runtime.GOOS == "darwin" {
				isMacSupported = true
			}

			games = append(games, models.LibraryGame{
				ID:                  appID,
				Name:                name,
				Platform:            "Steam",
				IconURL:             fmt.Sprintf("https://cdn.cloudflare.steamstatic.com/steam/apps/%s/header.jpg", appID),
				ExePath:             fullPath,
				AvailableOnAccounts: owners,
				IsInstalled:         true,
				IsMacSupported:      isMacSupported,
			})
		}
	}
	return games, nil
}

// ownedGamesFromConfigs извлекает владение играми из локальных userdata-конфигов
// аккаунта (localconfig.vdf, sharedconfig.vdf) — это оффлайн-зеркало списка
// покупок. Возвращает map[appID]имя (имя может быть пустым).
func (s *SteamScanner) ownedGamesFromConfigs(acc models.Account) map[string]string {
	result := make(map[string]string)
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

			if existing, found := result[appID]; !found || (existing == "" && gameName != "") {
				result[appID] = gameName
			}
		}
	}
	return result
}

// maxStoreNameLookups ограничивает количество одиночных запросов к Store API
// за один рескан: иначе большая библиотека без имен упрется в rate limit.
const maxStoreNameLookups = 25

// GetRemoteLibrary implements interfaces.RemoteLibraryProvider: купленные игры
// по каждому найденному аккаунту (IsInstalled=false; установка определяется
// при мерже с ScanInstalledGames). Источники владения:
//  1. Steam Community XML API — полный список покупок (нужен открытый профиль);
//  2. локальные userdata-конфиги — оффлайн-зеркало владения.
//
// Имена: XML/конфиг -> общий кэш имен -> Store API (с лимитом).
func (s *SteamScanner) GetRemoteLibrary() ([]models.LibraryGame, error) {
	if s.Path == "" {
		return nil, fmt.Errorf("steam path not found")
	}

	ensureGameNamesLoaded()
	accounts := s.GetAccounts()

	type ownedGame struct {
		name   string
		owners []models.AccountStat
	}
	owned := make(map[string]*ownedGame)
	var order []string // стабильный порядок появления
	cacheUpdated := false

	addOwned := func(appID, name string, acc models.Account) {
		if appID == "" {
			return
		}
		g, found := owned[appID]
		if !found {
			g = &ownedGame{}
			owned[appID] = g
			order = append(order, appID)
		}
		if g.name == "" && name != "" {
			g.name = name
		}
		for _, o := range g.owners {
			if o.AccountID == acc.ID {
				return
			}
		}
		g.owners = append(g.owners, models.AccountStat{
			AccountID:   acc.ID,
			DisplayName: acc.DisplayName,
			Username:    acc.Username,
		})
	}

	for _, acc := range accounts {
		// 1. Community XML API: полный список купленных игр аккаунта
		for _, g := range s.fetchOwnedGamesXML(acc) {
			addOwned(g.AppID, g.Name, acc)
			if g.AppID != "" && g.Name != "" {
				cacheMutex.Lock()
				if curr, exists := appNameCache[g.AppID]; !exists || strings.HasPrefix(curr, "Steam App") {
					appNameCache[g.AppID] = g.Name
					cacheUpdated = true
				}
				cacheMutex.Unlock()
			}
		}
		// 2. Локальные конфиги: работает оффлайн и при закрытом профиле
		for appID, name := range s.ownedGamesFromConfigs(acc) {
			addOwned(appID, name, acc)
		}
	}

	// Разрешение имен: кэш -> Store API (ограниченно)
	storeLookups := 0
	var games []models.LibraryGame
	for _, appID := range order {
		g := owned[appID]
		name := g.name
		if name == "" {
			cacheMutex.RLock()
			name = appNameCache[appID]
			cacheMutex.RUnlock()
		}
		if (name == "" || strings.HasPrefix(name, "Steam App")) && storeLookups < maxStoreNameLookups {
			storeLookups++
			if realName := fetchSingleGameName(appID); realName != "" {
				name = realName
				cacheMutex.Lock()
				appNameCache[appID] = realName
				cacheMutex.Unlock()
				cacheUpdated = true
			}
		}
		if name == "" {
			name = fmt.Sprintf("Steam App %s", appID)
		}

		games = append(games, models.LibraryGame{
			ID:                  appID,
			Name:                name,
			Platform:            "Steam",
			IconURL:             fmt.Sprintf("https://cdn.cloudflare.steamstatic.com/steam/apps/%s/header.jpg", appID),
			ExePath:             "",
			AvailableOnAccounts: g.owners,
			IsInstalled:         false,
		})
	}

	if cacheUpdated {
		saveCacheToFile()
	}
	return games, nil
}

// GetGames — прежний формат "все одним списком" (установленные + купленные).
// Сохранен для обратной совместимости; services/library использует
// гранулярные ScanInstalledGames/GetRemoteLibrary напрямую.
func (s *SteamScanner) GetGames() []models.LibraryGame {
	installed, err := s.ScanInstalledGames()
	if err != nil {
		return installed
	}
	remote, err := s.GetRemoteLibrary()
	if err != nil {
		return installed
	}
	return models.MergeLibraries(installed, remote)
}

// ... остальной код (checkMacSupport, getLibraryFolders и т.д.) без изменений ...

// Кэш совместимости (чтобы не спамить API при каждом запуске)
var osSupportCache = make(map[string]bool)

func checkMacSupport(appID string) bool {
	if val, ok := osSupportCache[appID]; ok {
		return val
	}
	url := fmt.Sprintf("https://store.steampowered.com/api/appdetails?appids=%s&filters=platforms", appID)
	resp, err := http.Get(url)
	if err != nil {
		return false
	}
	defer resp.Body.Close()

	var result map[string]storeAppDetailsResponse
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return false
	}
	if appData, ok := result[appID]; ok && appData.Success {
		isMac := appData.Data.Platforms.Mac
		osSupportCache[appID] = isMac
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
	localConfigPath := filepath.Join(s.Path, "userdata", steamID3, "config", "localconfig.vdf")
	if fileContainsAppID(localConfigPath, appID) {
		return true
	}
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

// RemoveUser удаляет пользователя из loginusers.vdf и его папку userdata
func (s *SteamScanner) RemoveUser(username string) error {
	if s.Path == "" {
		return fmt.Errorf("путь к Steam не найден")
	}

	accounts := s.GetAccounts()
	var targetID3 string
	for _, acc := range accounts {
		if acc.Username == username {
			targetID3 = acc.ID
			break
		}
	}

	if targetID3 == "" {
		return fmt.Errorf("аккаунт '%s' не найден", username)
	}

	// 1. Удаляем пользователя из loginusers.vdf
	loginUsersPath := filepath.Join(s.Path, "config", "loginusers.vdf")
	contentBytes, err := os.ReadFile(loginUsersPath)
	if err == nil {
		content := string(contentBytes)
		id3, _ := strconv.ParseInt(targetID3, 10, 64)
		id64 := id3 + 76561197960265728
		id64Str := strconv.FormatInt(id64, 10)

		// Этот регэксп удаляет весь блок пользователя.
		// (?s) позволяет `.` соответствовать новым строкам.
		re := regexp.MustCompile(fmt.Sprintf(`(?s)\s*"%s"\s*{[^}]+}`, id64Str))
		newContent := re.ReplaceAllString(content, "")

		if newContent != content {
			err = os.WriteFile(loginUsersPath, []byte(newContent), 0644)
			if err != nil {
				return fmt.Errorf("ошибка записи loginusers.vdf: %v", err)
			}
		}
	}

	// 2. Удаляем папку userdata
	userDataPath := filepath.Join(s.Path, "userdata", targetID3)
	if _, err := os.Stat(userDataPath); !os.IsNotExist(err) {
		return os.RemoveAll(userDataPath)
	}

	return nil
}

// GetName implements PlatformProvider
func (s *SteamScanner) GetName() string {
	return "Steam"
}

// GetGames Wrapper to match interface signature (interface expects error)
// The original GetGames didn't return error, so we wrap it.
func (s *SteamScanner) GetGamesProvider() ([]models.LibraryGame, error) {
	return s.GetGames(), nil
}

// GetAccountsWrapper to match interface signature
func (s *SteamScanner) GetAccountsProvider() ([]models.Account, error) {
	return s.GetAccounts(), nil
}

// SwitchAccount implements PlatformProvider.
// Legacy-путь "все в одном": kill -> activate -> start. Гранулярные шаги
// (KillLauncher/ActivateAccount/StartLauncher) живут в steam_session.go,
// их по отдельности оркеструет switcher.Service.
func (s *SteamScanner) SwitchAccount(accountName string) error {
	if accountName == "UNKNOWN" {
		return fmt.Errorf("login not found")
	}

	fmt.Printf("[Steam] Switching account (%s)...\n", runtime.GOOS)

	// 1. Kill Steam
	if err := s.KillLauncher(); err != nil {
		return fmt.Errorf("failed to kill steam: %w", err)
	}

	time.Sleep(1 * time.Second)

	// 2. Registry (AutoLoginUser) + LoginUsers VDF (MostRecent)
	if err := s.ActivateAccount(accountName); err != nil {
		return err
	}

	fmt.Println("[Steam] Configs updated. Launching Steam...")
	return s.StartLauncher()
}

// LaunchGame implements PlatformProvider
func (s *SteamScanner) LaunchGame(gameID string, exePath string) error {
	// Ensure correct user is active? Steam handles this usually if we just run...
	// But swch logic was to kill and switch if needed.
	// app.go: SwitchToAccount -> Launch.
	// Usually the UI calls Switch first.

	sys.StartGame("steam://run/" + gameID)
	return nil
}
