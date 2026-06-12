package bus

// Имена каналов событий Backend -> Frontend.
// Новый код обязан использовать эти константы вместо магических строк.
const (
	// Switcher (internal/services/switcher)
	EventSwitcherStatus = "switcher:status" // payload: switcher.Status — прогресс шагов
	EventSwitcherDone   = "switcher:done"   // payload: switcher.Status — итог (OK=true/false)

	// Library (internal/services/library)
	EventLibraryRefreshProgress = "library:refresh:progress" // payload: library.Progress — по источнику

	// Существующие каналы app.go. Значения совпадают с историческими строками,
	// чтобы не ломать текущий фронтенд; app.go мигрирует на константы в Phase A.
	EventClaimerLog     = "claimer-log"
	EventClaimerStatus  = "claimer-status"
	EventGameStarted    = "game-started"
	EventGameStopped    = "game-stopped"
	EventLibraryUpdated = "library-updated"
)
