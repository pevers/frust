import fs from 'fs';
import { getLogger } from './logging';

export const SETTINGS_PATH = "/home/pi/fridge.json";

export type Settings = {
  target_temp: number;
  status: "Idle" | "Cooling";
  p: number;
  i: number;
  d: number;
}

export const readSettings = (): Settings => {
  let json;
  json = fs.readFileSync(SETTINGS_PATH, 'utf-8');
  return JSON.parse(json) as Settings;
}

export const writeSettings = (settings: Settings) => {
  fs.writeFileSync(SETTINGS_PATH, JSON.stringify(settings, null, 4), 'utf-8');
}

export const updateSettings = (settings: Partial<Settings>) => {
  let currentSettings = readSettings();
  currentSettings = {
    ...currentSettings,
    ...settings
  };
  writeSettings(currentSettings);
}