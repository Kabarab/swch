// Package tracker реализует учёт игрового времени в стиле Playnite:
// после запуска игры наблюдает за её процессом и записывает длительность
// сессии, количество запусков и дату последнего запуска в настройки.
package tracker

import (
	"fmt"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"swch/internal/settings"
	"swch/internal/sys"
)

const (
	pollInterval  = 15 * time.Second
	startTimeout  = 5 * time.Minute // сколько ждём появления процесса игры после запуска
	maxSession    = 24 * time.Hour  // защита от «вечных» сессий
	flushInterval = 5 * time.Minute // как часто сохраняем прогресс длинной сессии на диск
)

type Tracker struct {
	settings *settings.Manager
	mu       sync.Mutex
	active   map[string]bool
	onUpdate func(gameID string)
}

func NewTracker(mgr *settings.Manager) *Tracker {
	return &Tracker{
		settings: mgr,
		active:   make(map[string]bool),
	}
}

// SetUpdateHandler задаёт колбэк, вызываемый после изменения статистики игры
// (используется для отправки события на фронтенд).
func (t *Tracker) SetUpdateHandler(fn func(gameID string)) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.onUpdate = fn
}

func (t *Tracker) notify(gameID string) {
	t.mu.Lock()
	fn := t.onUpdate
	t.mu.Unlock()
	if fn != nil {
		fn(gameID)
	}
}

// RecordLaunch фиксирует запуск игры (счётчик и дата) и, если известен путь
// к exe или папке установки, начинает отслеживание игрового времени.
func (t *Tracker) RecordLaunch(gameID, name, target string) {
	if gameID == "" {
		return
	}
	now := time.Now().Unix()
	t.settings.UpdateGameSettings(gameID, func(s *settings.GameSettings) {
		s.PlayCount++
		s.LastPlayed = now
	})
	t.settings.Save()
	t.notify(gameID)

	if target == "" {
		fmt.Printf("[Tracker] Для '%s' неизвестен путь установки — игровое время не отслеживается\n", name)
		return
	}
	t.startSession(gameID, name, target)
}

func (t *Tracker) startSession(gameID, name, target string) {
	t.mu.Lock()
	if t.active[gameID] {
		t.mu.Unlock()
		return // сессия для этой игры уже отслеживается
	}
	t.active[gameID] = true
	t.mu.Unlock()

	go t.watch(gameID, name, target)
}

func (t *Tracker) watch(gameID, name, target string) {
	defer func() {
		t.mu.Lock()
		delete(t.active, gameID)
		t.mu.Unlock()
	}()

	fmt.Printf("[Tracker] Наблюдение за '%s' (%s)\n", name, target)

	// 1. Ждём появления процесса (лаунчеры стартуют игру не мгновенно)
	deadline := time.Now().Add(startTimeout)
	started := time.Time{}
	for time.Now().Before(deadline) {
		if isGameRunning(target) {
			started = time.Now()
			break
		}
		time.Sleep(pollInterval)
	}
	if started.IsZero() {
		fmt.Printf("[Tracker] Процесс '%s' не появился за %v — сессия не записана\n", name, startTimeout)
		return
	}
	fmt.Printf("[Tracker] Сессия '%s' началась\n", name)

	// 2. Следим за процессом, периодически сохраняя прогресс,
	// чтобы время не потерялось при закрытии swch
	lastFlush := started
	for time.Since(started) < maxSession {
		time.Sleep(pollInterval)
		if !isGameRunning(target) {
			break
		}
		if time.Since(lastFlush) >= flushInterval {
			t.addPlaytime(gameID, int64(time.Since(lastFlush).Seconds()))
			lastFlush = time.Now()
		}
	}

	remaining := int64(time.Since(lastFlush).Seconds())
	t.addPlaytime(gameID, remaining)
	fmt.Printf("[Tracker] Сессия '%s' завершена: %v\n", name, time.Since(started).Round(time.Second))
}

func (t *Tracker) addPlaytime(gameID string, seconds int64) {
	if seconds <= 0 {
		return
	}
	t.settings.UpdateGameSettings(gameID, func(s *settings.GameSettings) {
		s.PlaytimeSeconds += seconds
	})
	t.settings.Save()
	t.notify(gameID)
}

// isGameRunning проверяет, запущен ли процесс игры.
// target — путь к exe-файлу либо к папке установки игры.
func isGameRunning(target string) bool {
	procs, err := sys.ListProcessPaths()
	if err != nil {
		return false
	}
	return matchProcess(procs, target)
}

// matchProcess сопоставляет список процессов с целевым exe/папкой установки.
// Совпадение: точный путь, процесс внутри папки установки либо имя exe-файла
// (на случай, когда система отдаёт только имена процессов).
func matchProcess(procs []string, target string) bool {
	norm := func(p string) string {
		return strings.ToLower(filepath.ToSlash(filepath.Clean(p)))
	}
	tgt := norm(target)
	base := strings.ToLower(filepath.Base(target))
	baseIsExe := strings.Contains(base, ".")

	for _, p := range procs {
		pn := norm(p)
		if pn == tgt || strings.HasPrefix(pn, tgt+"/") {
			return true
		}
		if baseIsExe && strings.ToLower(filepath.Base(pn)) == base {
			return true
		}
	}
	return false
}
