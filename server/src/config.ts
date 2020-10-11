import fs from 'fs';

export const CONTEXT_PATH = '/var/log/fridge-status.json';
export const CONFIG_PATH = '/etc/fridge.json';

export type Config = {
  target_temp: number;
  p: number;
  i: number;
  d: number;
};

export type Context = {
  inside_temp: number;
  outside_temp: number;
  correction: number;
  status: 'Idle' | 'Cooling';
  config: Config;
};

export const readContext = (): Context => {
  let json;
  json = fs.readFileSync(CONTEXT_PATH, 'utf-8');
  return JSON.parse(json) as Context;
};

export const writeConfig = (config: Config) => {
  fs.writeFileSync(CONFIG_PATH, JSON.stringify(config, null, 4), 'utf-8');
};

export const updateConfig = (config: Partial<Config>) => {
  let currentConfig = readContext().config;
  currentConfig = {
    target_temp: Number(config.target_temp || currentConfig.target_temp),
    p: Number(config.p || currentConfig.p),
    i: Number(config.i || currentConfig.i),
    d: Number(config.d || currentConfig.d),
  };
  writeConfig(currentConfig);
};
