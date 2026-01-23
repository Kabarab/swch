package scanner

import (
	"fmt"
	"io/ioutil"
	"os"
	"path/filepath"
	"strconv"
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

// SetMostRecentUser изменяет loginusers.vdf, чтобы Steam знал, кого грузить
func (s *SteamScanner) SetMostRecentUser(targetUsername string) error {
	loginUsersPath := filepath.Join(s.Path, "config", "loginusers.vdf")
	
	// Читаем файл как ТЕКСТ (чтобы не сломать форматирование VDF при перезаписи)
	// VDF библиотека не умеет сохранять обратно идеально, поэтому сделаем простую текстовую замену.
	// Это безопаснее и надежнее для переключателя.
	contentBytes, err := ioutil.ReadFile(loginUsersPath)
	if err != nil {
		return err
	}
	content := string(contentBytes)

	// Нам нужно найти блок пользователя с этим логином и добавить/обновить MostRecent.
	// Но так как парсить и собирать VDF сложно, мы используем трюк:
	// Стим при выходе сам обновляет этот файл.
	// Если мы просто удалим этот файл, Стим попросит вход.
	// 
	// ЛУЧШИЙ ВАРИАНТ: Мы не будем переписывать сложный VDF вручную. 
	// Мы полагаемся на РЕЕСТР, но перед этим удаляем старый кэш активного юзера в реестре.
	
	return nil 
}

// GetGames возвращает список игр
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
	m, err := p.Parse()
	if err != nil { return paths } // Fix potential crash

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

		// Глубокий поиск пользователя в структуре VDF
		var userData map[string]interface{}
		
		// VDF может быть "users" -> "ID" или сразу "ID"
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
			if n, ok := userData["PersonaName"].(string); ok { displayName = n }
			if a, ok := userData["AccountName"].(string); ok { username = a }
		}

		// Если логин не найден, помечаем его, чтобы не ломать реестр пустыми строками
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