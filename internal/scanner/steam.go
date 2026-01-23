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

// SetUserActive реализует логику TcNo: ставит MostRecent=1 И обновляет Timestamp
func (s *SteamScanner) SetUserActive(targetUsername string) error {
	loginUsersPath := filepath.Join(s.Path, "config", "loginusers.vdf")
	
	contentBytes, err := ioutil.ReadFile(loginUsersPath)
	if err != nil { return err }
	content := string(contentBytes)

	// 1. Сбрасываем "MostRecent" "1" у всех на "0"
	reReset := regexp.MustCompile(`"MostRecent"\s+"1"`)
	content = reReset.ReplaceAllString(content, `"MostRecent"		"0"`)

	// 2. Ищем нашего пользователя
	targetPattern := fmt.Sprintf(`"(?i)%s"`, regexp.QuoteMeta(targetUsername))
	loc := regexp.MustCompile(targetPattern).FindStringIndex(content)
	
	if loc != nil {
		// Область поиска параметров для этого юзера (от его имени до конца файла)
		// (В идеале надо искать до следующей закрывающей скобки, но для VDF это обычно работает)
		restOfFile := content[loc[1]:]
		
		// --- АПДЕЙТ 1: MostRecent ---
		reMostRecent := regexp.MustCompile(`"MostRecent"\s+"0"`)
		locRecent := reMostRecent.FindStringIndex(restOfFile)
		
		if locRecent != nil {
			startPos := loc[1] + locRecent[0]
			endPos := loc[1] + locRecent[1]
			// Меняем 0 на 1
			content = content[:startPos] + `"MostRecent"		"1"` + content[endPos:]
			
			// Перечитываем restOfFile, так как длина строки могла измениться (хотя тут замена 1 к 1)
			restOfFile = content[loc[1]:] 
		}

		// --- АПДЕЙТ 2: Timestamp (КЛЮЧЕВОЙ МОМЕНТ TCNO) ---
		// Ищем поле Timestamp
		reTime := regexp.MustCompile(`"Timestamp"\s+"(\d+)"`)
		locTime := reTime.FindStringSubmatchIndex(restOfFile)

		if locTime != nil {
			// locTime[2] и locTime[3] - это индексы самой цифры времени
			startPos := loc[1] + locTime[2]
			endPos := loc[1] + locTime[3]
			
			// Текущее время Unix
			newTime := fmt.Sprintf("%d", time.Now().Unix())
			
			// Заменяем старое время на новое
			content = content[:startPos] + newTime + content[endPos:]
		}
		
		return ioutil.WriteFile(loginUsersPath, []byte(content), 0644)
	}
	
	return fmt.Errorf("user not found in VDF")
}

// GetGames возвращает игры
func (s *SteamScanner) GetGames() []models.LibraryGame {
	var games []models.LibraryGame
	if s.Path == "" { return games }

	accounts := s.GetAccounts()
	libraryPaths := s.getLibraryFolders()

	for _, libPath := range libraryPaths {
		steamAppsPath := filepath.Join(libPath, "steamapps")
		files, err := os.ReadDir(steamAppsPath)
		if err != nil { continue }

		for _, f := range files {
			if strings.HasPrefix(f.Name(), "appmanifest_") && strings.HasSuffix(f.Name(), ".acf") {
				data := parseVdf(filepath.Join(steamAppsPath, f.Name()))
				appState, ok := data["AppState"].(map[string]interface{})
				if !ok { continue }

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
	if err != nil { return paths }
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
		if !entry.IsDir() { continue }
		steamID3 := entry.Name()
		if _, err := strconv.Atoi(steamID3); err != nil { continue }

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
			// Пробуем искать в корне (старый формат или без users)
			if u, found := loginData[id64Str].(map[string]interface{}); found {
				userData = u
			}
		}

		if userData != nil {
			if n, ok := userData["PersonaName"].(string); ok { displayName = n }
			if a, ok := userData["AccountName"].(string); ok { username = a }
		}

		if username == "" { username = "UNKNOWN" }

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
	if err != nil { return false }
	return strings.Contains(string(contentBytes), fmt.Sprintf(`"%s"`, appID))
}

func parseVdf(path string) map[string]interface{} {
	f, err := os.Open(path)
	if err != nil { return nil }
	defer f.Close()
	p := vdf.NewParser(f)
	m, _ := p.Parse()
	return m
}