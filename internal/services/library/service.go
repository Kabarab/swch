// Package library — воркер агрегации игровых библиотек (концепция Playnite).
//
// Для каждого провайдера собираются два источника:
//   - LocalLibraryScanner: установленные игры из локальных папок/манифестов
//     (steamapps/appmanifest_*.acf, EGS Data/Manifests/*.item);
//   - RemoteLibraryProvider: купленные игры через API магазина (токены из
//     сохраненных сессий модуля аккаунтов);
//
// Провайдеры без гранулярных способностей опрашиваются через legacy
// GetGamesProvider. Результаты сводятся в единый формат models.LibraryGame
// (ID, Название, Платформа, Статус установки, Путь) и кэшируются: UI всегда
// читает кэш мгновенно, пересканирование идет в фоне.
package library

import (
	"fmt"
	"sync"
	"time"

	"swch/internal/bus"
	"swch/internal/interfaces"
	"swch/internal/models"
)

// refreshDebounce защищает от двойного скана при старте и спама кнопкой
// Refresh: повторный запрос сразу после завершения вернет кэш.
const refreshDebounce = 10 * time.Second

// Источники данных (поле Progress.Source).
const (
	SourceLocal    = "local"    // локальные манифесты
	SourceRemote   = "remote"   // API магазина
	SourceProvider = "provider" // legacy GetGamesProvider
)

// Progress — payload события bus.EventLibraryRefreshProgress.
type Progress struct {
	Platform string `json:"platform"`
	Source   string `json:"source"`
	Count    int    `json:"count"`
	Error    string `json:"error,omitempty"`
}

type Service struct {
	providers map[string]interfaces.PlatformProvider
	emitter   bus.Emitter
	cache     *Cache
	refreshMu sync.Mutex // один Refresh одновременно
}

// NewService создает сервис. cachePath — путь к JSON-кэшу
// (пустая строка = кэш только в памяти).
func NewService(providers map[string]interfaces.PlatformProvider, emitter bus.Emitter, cachePath string) *Service {
	if emitter == nil {
		emitter = bus.NullEmitter{}
	}
	return &Service{
		providers: providers,
		emitter:   emitter,
		cache:     NewCache(cachePath),
	}
}

// Get возвращает библиотеку из кэша. Пустой кэш (самый первый запуск) —
// синхронный Refresh, чтобы UI получил данные, а не пустой экран.
func (s *Service) Get() []models.LibraryGame {
	if s.cache.IsEmpty() {
		return s.Refresh()
	}
	return s.cache.Get()
}

// Refresh синхронно пересканирует всех провайдеров и обновляет кэш.
func (s *Service) Refresh() []models.LibraryGame {
	s.refreshMu.Lock()
	defer s.refreshMu.Unlock()
	return s.refreshLocked()
}

// RefreshAsync запускает Refresh в фоне; если обновление уже идет,
// повторный запуск не создается. По завершении уйдет library-updated.
func (s *Service) RefreshAsync() {
	if !s.refreshMu.TryLock() {
		return
	}
	go func() {
		defer s.refreshMu.Unlock()
		s.refreshLocked()
	}()
}

func (s *Service) refreshLocked() []models.LibraryGame {
	if !s.cache.IsEmpty() && time.Since(s.cache.UpdatedAt()) < refreshDebounce {
		return s.cache.Get()
	}

	var (
		mu    sync.Mutex
		lists [][]models.LibraryGame
		wg    sync.WaitGroup
	)
	// Провайдеры опрашиваются параллельно (как делал app.GetLibrary)
	for name, provider := range s.providers {
		wg.Add(1)
		go func(platform string, p interfaces.PlatformProvider) {
			defer wg.Done()
			collected := s.collect(platform, p)
			mu.Lock()
			lists = append(lists, collected...)
			mu.Unlock()
		}(name, provider)
	}
	wg.Wait()

	merged := models.MergeLibraries(lists...)
	s.cache.Set(merged)
	fmt.Printf("[Library] Refresh complete: %d games\n", len(merged))
	s.emitter.Emit(bus.EventLibraryUpdated)
	return s.cache.Get()
}

// collect собирает игры одного провайдера из всех доступных ему источников.
func (s *Service) collect(platform string, p interfaces.PlatformProvider) [][]models.LibraryGame {
	local, hasLocal := p.(interfaces.LocalLibraryScanner)
	remote, hasRemote := p.(interfaces.RemoteLibraryProvider)

	if !hasLocal && !hasRemote {
		games, err := p.GetGamesProvider()
		s.emitProgress(platform, SourceProvider, games, err)
		if err != nil {
			return nil
		}
		return [][]models.LibraryGame{games}
	}

	var out [][]models.LibraryGame
	if hasLocal {
		games, err := local.ScanInstalledGames()
		s.emitProgress(platform, SourceLocal, games, err)
		if err == nil {
			out = append(out, games)
		}
	}
	if hasRemote {
		games, err := remote.GetRemoteLibrary()
		s.emitProgress(platform, SourceRemote, games, err)
		if err == nil {
			out = append(out, games)
		}
	}
	return out
}

func (s *Service) emitProgress(platform, source string, games []models.LibraryGame, err error) {
	p := Progress{Platform: platform, Source: source, Count: len(games)}
	if err != nil {
		p.Error = err.Error()
		fmt.Printf("[Library] %s (%s): %v\n", platform, source, err)
	}
	s.emitter.Emit(bus.EventLibraryRefreshProgress, p)
}
