package main

import (
	"fmt"
	"os/exec"
	"strings"
)

func main() {
	fmt.Println("Scanning Keychain for Epic-related entries...")

	// Dump keychain to stdout
	out, err := exec.Command("security", "dump-keychain").Output()
	if err != nil {
		fmt.Printf("Error dumping keychain: %v\n", err)
		fmt.Println("Try running this with 'sudo' or ensure you allow access if prompted.")
		return
	}

	lines := strings.Split(string(out), "\n")
	for i, line := range lines {
		// Look for Service Name attribute "svce"
		if strings.Contains(line, "\"svce\"<blob>=") {
			// Extract the value (it's often followed by the string in quotes or hex)
			// Simple grep logic context
			if i > 5 && i < len(lines)-5 {
				context := strings.Join(lines[i-5:i+5], "\n")
				if strings.Contains(strings.ToLower(context), "epic") ||
					strings.Contains(strings.ToLower(context), "unreal") ||
					strings.Contains(strings.ToLower(context), "launcher") {
					fmt.Println("---------------------------------------------------")
					fmt.Println("FOUND CANDIDATE:")
					fmt.Println(context)
					fmt.Println("---------------------------------------------------")
				}
			}
		}
	}
}
