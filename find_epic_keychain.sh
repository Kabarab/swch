#!/bin/bash
echo "=== Epic Games Keychain Diagnostic (Enhanced) ==="
echo "Searching for keys..."

echo "1. Checking Generic Passwords..."
security find-generic-password -l "Epic Games Launcher"
security find-generic-password -s "EpicGamesLauncher"
security find-generic-password -s "com.epicgames.EpicGamesLauncher"
security find-generic-password -s "Epic"
security find-generic-password -s "Unreal"

echo "2. Checking Internet Passwords..."
security find-internet-password -s "EpicGames"
security find-internet-password -s "EpicGamesLauncher"
security find-internet-password -s "www.epicgames.com"

echo "3. Broad search (dump & grep)..."
# Dump valid keychain items and search for keywords
security dump-keychain | grep -i -A 4 -B 4 "Epic"
security dump-keychain | grep -i -A 4 -B 4 "Unreal"

echo "=== End of Diagnostic ==="
