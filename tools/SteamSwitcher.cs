using Microsoft.Win32;
using System;
using System.Diagnostics;
using System.IO;
using System.Text.RegularExpressions;
using System.Threading;

public class Program
{
    public static void Main(string[] args)
    {
        if (args.Length == 0)
        {
            Console.WriteLine("Error: No username provided.");
            return;
        }

        string targetUser = args[0];
        string gameId = (args.Length > 1) ? args[1] : null;

        string steamPath = GetSteamPath();
        if (string.IsNullOrEmpty(steamPath))
        {
             // Fallback paths
             if (Directory.Exists("C:\\Program Files (x86)\\Steam"))
                 steamPath = "C:\\Program Files (x86)\\Steam";
             else if (Directory.Exists("C:\\Program Files\\Steam"))
                 steamPath = "C:\\Program Files\\Steam";
             else
             {
                 Console.WriteLine("Error: Steam path not found.");
                 return;
             }
        }

        Console.WriteLine("[1/4] Switching to: " + targetUser);
        KillSteam();

        Console.WriteLine("[2/4] Patching loginusers.vdf...");
        PatchVdf(steamPath, targetUser);

        Console.WriteLine("[3/4] Updating Registry...");
        SetRegistry(targetUser);

        Console.WriteLine("[4/4] Starting Steam...");
        StartSteam(steamPath, gameId);
    }

    static string GetSteamPath()
    {
        try 
        {
            using (RegistryKey key = Registry.CurrentUser.OpenSubKey("Software\\Valve\\Steam"))
            {
                if (key != null)
                {
                    object val = key.GetValue("SteamPath");
                    if (val != null) return val.ToString().Replace("/", "\\");
                }
            }
        }
        catch {}
        return null;
    }

    static void KillSteam()
    {
        string[] procs = { "steam", "steamwebhelper", "GameOverlayUI" };
        foreach (string procName in procs)
        {
            foreach (Process proc in Process.GetProcessesByName(procName))
            {
                try { proc.Kill(); } catch { }
            }
        }
        Thread.Sleep(1500);
    }

    static void SetRegistry(string username)
    {
        string keyPath = "Software\\Valve\\Steam";
        try
        {
            using (RegistryKey key = Registry.CurrentUser.OpenSubKey(keyPath, true))
            {
                if (key != null)
                {
                    key.SetValue("AutoLoginUser", username, RegistryValueKind.String);
                    key.SetValue("RememberPassword", 1, RegistryValueKind.DWord);
                    key.SetValue("SkipOfflineModeWarning", 1, RegistryValueKind.DWord);
                }
            }

            using (RegistryKey key = Registry.CurrentUser.OpenSubKey(keyPath + "\\ActiveProcess", true))
            {
                if (key != null)
                {
                    key.SetValue("ActiveUser", 0, RegistryValueKind.DWord);
                }
            }
        }
        catch (Exception ex)
        {
            Console.WriteLine("Registry Error: " + ex.Message);
        }
    }

    static void PatchVdf(string steamPath, string targetUsername)
    {
        string vdfPath = Path.Combine(steamPath, "config", "loginusers.vdf");
        if (!File.Exists(vdfPath)) return;

        try
        {
            string content = File.ReadAllText(vdfPath);

            // 1. Reset ALL "MostRecent" to "0" globally first
            content = Regex.Replace(content, "\"MostRecent\"\\s+\"1\"", "\"MostRecent\"      \"0\"");

            // 2. Find the specific user block using Regex
            string userBlockPattern = "(\\\"" + "\\d{17}" + "\\\"\\s*\\{[^{}]*\\\"AccountName\\\"\\s*\\\"" + Regex.Escape(targetUsername) + "\\\"[^{}]*\\})";
            
            content = Regex.Replace(content, userBlockPattern, new MatchEvaluator(delegate(Match match)
            {
                string block = match.Value;
                
                // Calculate timestamp
                TimeSpan t = DateTime.UtcNow - new DateTime(1970, 1, 1);
                int now = (int)t.TotalSeconds;

                // Update or Add "MostRecent"
                if (block.Contains("\"MostRecent\""))
                    block = Regex.Replace(block, "\"MostRecent\"\\s+\"0\"", "\"MostRecent\"      \"1\"");
                else
                    block = block.Insert(block.LastIndexOf('}'), "\t\"MostRecent\"      \"1\"\n\t");

                // Update or Add "Timestamp"
                if (block.Contains("\"Timestamp\""))
                    block = Regex.Replace(block, "\"Timestamp\"\\s+\"\\d+\"", "\"Timestamp\"      \"" + now + "\"");
                else
                    block = block.Insert(block.LastIndexOf('}'), "\t\"Timestamp\"      \"" + now + "\"\n\t");

                // Update or Add "AllowAutoLogin"
                if (block.Contains("\"AllowAutoLogin\""))
                    block = Regex.Replace(block, "\"AllowAutoLogin\"\\s+\"0\"", "\"AllowAutoLogin\"      \"1\"");
                else if (!block.Contains("\"AllowAutoLogin\""))
                    block = block.Insert(block.LastIndexOf('}'), "\t\"AllowAutoLogin\"      \"1\"\n\t");

                return block;
            }), RegexOptions.IgnoreCase | RegexOptions.Singleline);

            File.WriteAllText(vdfPath, content);
        }
        catch (Exception ex)
        {
            Console.WriteLine("VDF Patch Error: " + ex.Message);
        }
    }

    static void StartSteam(string steamPath, string appId)
    {
        string exe = Path.Combine(steamPath, "steam.exe");
        string args = (string.IsNullOrEmpty(appId)) ? "" : "-applaunch " + appId;
        
        try
        {
            Process.Start(new ProcessStartInfo
            {
                FileName = exe,
                Arguments = args,
                UseShellExecute = true,
                WorkingDirectory = steamPath
            });
        }
        catch (Exception ex)
        {
            Console.WriteLine("Start Error: " + ex.Message);
        }
    }
}