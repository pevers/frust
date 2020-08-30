import { createLogger, format, Logger, transports } from 'winston';

const theFormat = process.env.NODE_ENV === 'production' ? format.json() : format.simple();
const loggerCache = new Map<NodeModule, Logger>();
export const getLogger = (module: NodeModule) => {
  if (loggerCache.has(module)) {
    return loggerCache.get(module)!;
  }
  const path = module.filename
    .split('/')
    .slice(-2)
    .join('/');
  const newLogger = createLogger({
    level: process.env.LOG_LEVEL || 'info',
    format: theFormat,
    defaultMeta: {
      logger_name: path,
      thread_name: `${process.pid}`,
    },
    transports: [new transports.Console()],
  });
  loggerCache.set(module, newLogger);
  return newLogger;
};
