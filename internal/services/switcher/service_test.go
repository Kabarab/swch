package switcher

import (
	"fmt"
	"strings"
	"testing"

	"swch/internal/bus"
	"swch/internal/interfaces"
	"swch/internal/models"
)

// fakeProvider реализует все capability-интерфейсы и записывает порядок вызовов.
type fakeProvider struct {
	calls       []string
	failKill    bool
	failRestore bool
	failStart   bool
}

func (f *fakeProvider) GetName() string                                 { return "Fake" }
func (f *fakeProvider) GetGamesProvider() ([]models.LibraryGame, error) { return nil, nil }
func (f *fakeProvider) GetAccountsProvider() ([]models.Account, error)  { return nil, nil }
func (f *fakeProvider) LaunchGame(gameID, exePath string) error         { return nil }

func (f *fakeProvider) SwitchAccount(name string) error {
	f.calls = append(f.calls, "SwitchAccount")
	return nil
}

func (f *fakeProvider) KillLauncher() error {
	f.calls = append(f.calls, "KillLauncher")
	if f.failKill {
		return fmt.Errorf("kill failed")
	}
	return nil
}

func (f *fakeProvider) StartLauncher() error {
	f.calls = append(f.calls, "StartLauncher")
	if f.failStart {
		return fmt.Errorf("start failed")
	}
	return nil
}

func (f *fakeProvider) ActivateAccount(name string) error {
	f.calls = append(f.calls, "ActivateAccount")
	return nil
}

func (f *fakeProvider) SaveSession(name string) error {
	f.calls = append(f.calls, "SaveSession")
	return nil
}

func (f *fakeProvider) RestoreSession(name string) error {
	f.calls = append(f.calls, "RestoreSession")
	if f.failRestore {
		return fmt.Errorf("restore failed")
	}
	return nil
}

func (f *fakeProvider) ListSessions() ([]string, error) { return []string{"acc1"}, nil }
func (f *fakeProvider) DeleteSession(name string) error {
	f.calls = append(f.calls, "DeleteSession")
	return nil
}

// legacyProvider реализует только базовый PlatformProvider (как Battle.net и др.)
type legacyProvider struct {
	switchedTo string
}

func (l *legacyProvider) GetName() string                                 { return "Legacy" }
func (l *legacyProvider) GetGamesProvider() ([]models.LibraryGame, error) { return nil, nil }
func (l *legacyProvider) GetAccountsProvider() ([]models.Account, error)  { return nil, nil }
func (l *legacyProvider) LaunchGame(gameID, exePath string) error         { return nil }
func (l *legacyProvider) SwitchAccount(name string) error {
	l.switchedTo = name
	return nil
}

// recordingEmitter записывает имена событий для проверок.
type recordingEmitter struct {
	events []string
}

func (r *recordingEmitter) Emit(event string, data ...interface{}) {
	r.events = append(r.events, event)
}

func newTestService(p interfaces.PlatformProvider, emitter bus.Emitter) *Service {
	svc := NewService(map[string]interfaces.PlatformProvider{"Fake": p}, emitter)
	svc.killDelay = 0 // в тестах не ждем освобождения файловых блокировок
	return svc
}

func assertCalls(t *testing.T, got []string, want ...string) {
	t.Helper()
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Errorf("call order mismatch:\n got:  %v\n want: %v", got, want)
	}
}

func TestSwitch_FullCycleOrder(t *testing.T) {
	p := &fakeProvider{}
	emitter := &recordingEmitter{}
	svc := newTestService(p, emitter)

	if err := svc.Switch("Fake", "player1"); err != nil {
		t.Fatalf("Switch failed: %v", err)
	}
	// TcNo-цикл: kill -> activate -> start (без подмены файлов)
	assertCalls(t, p.calls, "KillLauncher", "ActivateAccount", "StartLauncher")

	last := emitter.events[len(emitter.events)-1]
	if last != bus.EventSwitcherDone {
		t.Errorf("last event = %s, want %s", last, bus.EventSwitcherDone)
	}
}

func TestSwitch_LegacyFallback(t *testing.T) {
	p := &legacyProvider{}
	svc := NewService(map[string]interfaces.PlatformProvider{"Fake": p}, nil)

	if err := svc.Switch("Fake", "player1"); err != nil {
		t.Fatalf("Switch failed: %v", err)
	}
	if p.switchedTo != "player1" {
		t.Errorf("legacy SwitchAccount not called, switchedTo=%q", p.switchedTo)
	}
}

func TestSwitch_UnknownPlatform(t *testing.T) {
	svc := NewService(map[string]interfaces.PlatformProvider{}, nil)
	if err := svc.Switch("Nope", "player1"); err == nil {
		t.Fatal("expected error for unknown platform")
	}
}

func TestSwitch_KillFailureAborts(t *testing.T) {
	p := &fakeProvider{failKill: true}
	svc := newTestService(p, nil)

	if err := svc.Switch("Fake", "player1"); err == nil {
		t.Fatal("expected error when kill fails")
	}
	// После неудачного kill активация не должна выполняться
	assertCalls(t, p.calls, "KillLauncher")
}

func TestRestoreSession_FullCycleOrder(t *testing.T) {
	p := &fakeProvider{}
	svc := newTestService(p, nil)

	if err := svc.RestoreSession("Fake", "player1"); err != nil {
		t.Fatalf("RestoreSession failed: %v", err)
	}
	// Подмена файлов строго между kill и activate
	assertCalls(t, p.calls, "KillLauncher", "RestoreSession", "ActivateAccount", "StartLauncher")
}

func TestRestoreSession_FailureSkipsStart(t *testing.T) {
	p := &fakeProvider{failRestore: true}
	emitter := &recordingEmitter{}
	svc := newTestService(p, emitter)

	if err := svc.RestoreSession("Fake", "player1"); err == nil {
		t.Fatal("expected error when restore fails")
	}
	assertCalls(t, p.calls, "KillLauncher", "RestoreSession")

	last := emitter.events[len(emitter.events)-1]
	if last != bus.EventSwitcherDone {
		t.Errorf("failure must emit done event with OK=false, got %s", last)
	}
}

func TestRestoreSession_NotSupported(t *testing.T) {
	p := &legacyProvider{}
	svc := NewService(map[string]interfaces.PlatformProvider{"Fake": p}, nil)

	if err := svc.RestoreSession("Fake", "player1"); err == nil {
		t.Fatal("expected error for provider without SessionSnapshotter")
	}
}

func TestSaveCurrentSession(t *testing.T) {
	p := &fakeProvider{}
	svc := newTestService(p, nil)

	if err := svc.SaveCurrentSession("Fake", "player1"); err != nil {
		t.Fatalf("SaveCurrentSession failed: %v", err)
	}
	assertCalls(t, p.calls, "SaveSession")
}

func TestValidateAccountName(t *testing.T) {
	bad := []string{"", "  ", ".", "..", "a/b", `a\b`}
	for _, name := range bad {
		if err := ValidateAccountName(name); err == nil {
			t.Errorf("ValidateAccountName(%q) = nil, want error", name)
		}
	}
	good := []string{"player1", "my_account", "Acc-2", "имя"}
	for _, name := range good {
		if err := ValidateAccountName(name); err != nil {
			t.Errorf("ValidateAccountName(%q) = %v, want nil", name, err)
		}
	}
}
