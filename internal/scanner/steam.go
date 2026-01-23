package scanner

import (
	"fmt"
	"io/ioutil"
	"os"
	"path/filepath"
	"strings"
	"swch/internal/models"
	"swch/internal/sys"

	"github.com/andygrunwald/vdf"
)

type SteamScanner struct {
	Path string
}

func NewSteamScanner() *SteamScanner {
	path, _ := sys.GetSteamPath()
	return &SteamScanner{Path: path}
}

// Scan находит аккаунты и их игры
func (s *SteamScanner) Scan() []models.Account {
	var accounts []models.Account
	if s.Path == "" {
		return accounts
	}

	// 1. Находим установленные игры (читаем appmanifest_*.acf)
	installedGames := s.scanInstalledGames()

	// 2. Читаем список пользователей из config/loginusers.vdf
	loginUsersPath := filepath.Join(s.Path, "config", "loginusers.vdf")
	loginData := parseVdf(loginUsersPath)

	// 3. Сканируем папку userdata для поиска локальных конфигов
	userDataPath := filepath.Join(s.Path, "userdata")
	entries, _ := os.ReadDir(userDataPath)

	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		steamID3 := entry.Name()
		if steamID3 == "0" || steamID3 == "anonymous" {
			continue
		}

		// Определяем имя пользователя
		displayName := "User " + steamID3
		loginName := ""

		// Простой поиск имени в loginusers.vdf
		for _, info := range loginData {
			if m, ok := info.(map[string]interface{}); ok {
				// В реальном проекте здесь нужна конвертация ID, но для MVP ищем по совпадению
				if name, ok := m["PersonaName"]; ok {
					// Если бы мы конвертировали ID, мы бы точно знали имя.
					// Тут мы просто берем имя, если это единственный юзер, или оставляем ID
					_ = name 
				}
				if acc, ok := m["AccountName"]; ok {
					loginName = acc.(string)
				}
			}
		}
		
		// Если нашли логин (AccountName), используем его для отображения, если нет PersonaName
		if loginName != "" {
			displayName = loginName
		} else {
			// Пытаемся вычитать имя из localconfig.vdf (иногда оно там есть)
			// Но для простоты оставим ID, если не нашли.
		}

		// 4. Фильтруем игры для этого аккаунта
		myGames := s.filterGamesForAccount(steamID3, installedGames)

		acc := models.Account{
			ID:          steamID3,
			DisplayName: displayName,
			Platform:    "Steam",
			OwnedGames:  myGames,
		}
		
		// Добавляем аккаунт, если нашли игры
		if len(myGames) > 0 {
			accounts = append(accounts, acc)
		}
	}
	return accounts
}

func (s *SteamScanner) scanInstalledGames() map[string]string {
	games := make(map[string]string)
	steamAppsPath := filepath.Join(s.Path, "steamapps")
	
	files, err := os.ReadDir(steamAppsPath)
	if err != nil { return games }

	for _, f := range files {
		if strings.HasPrefix(f.Name(), "appmanifest_") && strings.HasSuffix(f.Name(), ".acf") {
			fullPath := filepath.Join(steamAppsPath, f.Name())
			data := parseVdf(fullPath)
			
			if appState, ok := data["AppState"].(map[string]interface{}); ok {
				name := appState["name"].(string)
				appid := appState["appid"].(string)
				games[appid] = name
			}
		}
	}
	return games
}

func (s *SteamScanner) filterGamesForAccount(steamID3 string, installedGames map[string]string) []models.Game {
	var myGames []models.Game
	localConfigPath := filepath.Join(s.Path, "userdata", steamID3, "config", "localconfig.vdf")
	
	// Читаем файл как текст для быстрого поиска
	contentBytes, err := ioutil.ReadFile(localConfigPath)
	if err != nil { return myGames }
	content := string(contentBytes)

	for appID, name := range installedGames {
		// Если ID игры встречается в конфиге пользователя, считаем, что игра его
		if strings.Contains(content, fmt.Sprintf(`"%s"`, appID)) {
			myGames = append(myGames, models.Game{
				ID:       appID,
				Name:     name,
				Platform: "Steam",
				ImageURL: fmt.Sprintf("https://cdn.cloudflare.steamstatic.com/steam/apps/%s/header.jpg", appID),
			})
		}
	}
	return myGames
}

func parseVdf(path string) map[string]interface{} {
	f, err := os.Open(path)
	if err != nil { return nil }
	defer f.Close()
	p := vdf.NewParser(f)
	m, _ := p.Parse()
	return m
}