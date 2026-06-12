package tracker

import "testing"

func TestMatchProcess(t *testing.T) {
	cases := []struct {
		name   string
		procs  []string
		target string
		want   bool
	}{
		{
			name:   "exact exe path",
			procs:  []string{`C:\Games\Witcher3\bin\x64\witcher3.exe`},
			target: `C:\Games\Witcher3\bin\x64\witcher3.exe`,
			want:   true,
		},
		{
			name:   "process inside install dir",
			procs:  []string{`C:\Steam\steamapps\common\Dota 2\game\bin\win64\dota2.exe`},
			target: `C:\Steam\steamapps\common\Dota 2`,
			want:   true,
		},
		{
			name:   "exe name only (tasklist fallback)",
			procs:  []string{"witcher3.exe", "explorer.exe"},
			target: `C:\Games\Witcher3\witcher3.exe`,
			want:   true,
		},
		{
			name:   "case insensitive",
			procs:  []string{`c:\games\witcher3\WITCHER3.EXE`},
			target: `C:\Games\Witcher3\witcher3.exe`,
			want:   true,
		},
		{
			name:   "no match",
			procs:  []string{`C:\Windows\explorer.exe`},
			target: `C:\Games\Witcher3`,
			want:   false,
		},
		{
			name:   "dir name must not match by basename",
			procs:  []string{"Witcher3"},
			target: `C:\Games\Witcher3`,
			want:   false,
		},
		{
			name:   "macos app inside install dir",
			procs:  []string{"/Users/u/Library/Application Support/Steam/steamapps/common/Hades/Hades.app/Contents/MacOS/Hades"},
			target: "/Users/u/Library/Application Support/Steam/steamapps/common/Hades",
			want:   true,
		},
		{
			name:   "prefix must respect path boundary",
			procs:  []string{`C:\Games\Witcher3Mod\tool.exe`},
			target: `C:\Games\Witcher3`,
			want:   false,
		},
	}

	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := matchProcess(c.procs, c.target); got != c.want {
				t.Errorf("matchProcess(%v, %q) = %v, want %v", c.procs, c.target, got, c.want)
			}
		})
	}
}
