import { createTerminus } from '@godaddy/terminus';
import errorHandler from 'errorhandler';

import app from './app';
import { getLogger } from './logging';
import { readContext } from './config';
import Recorder, { StatusUpdate } from './recorder';

const log = getLogger(module);

const http = require('http').Server(app);
const io = require('socket.io')(http, { origins: '*:*' });

// Initialize the recorder
const recorder = new Recorder();
recorder.setupRecorder();
console.log('Recorder initialized');

/**
 * Error Handler. Provides full stack - remove for production
 */
if (process.env.NODE_ENV === 'development') {
  // only use in development
  app.use(errorHandler());
}

const server = http.listen(app.get('port'), () => {
  log.info(`App is running at http://localhost:${app.get('port')} in ${app.get('env')} mode`);
});

async function onSignal() {
  log.info('server is starting cleanup');
}

async function onHealthCheck() {
  return true;
}

// Start recording status updatess and emit them to the user
setInterval(async () => {
  const context = readContext();
  const status: StatusUpdate = {
    timestamp: new Date().toISOString(),
    inside_temp: context.inside_temp,
    outside_temp: context.outside_temp,
    target_temp: context.config.target_temp,
    correction: context.correction,
    status: context.status,
    p: context.config.p,
    i: context.config.i,
    d: context.config.d,
  };
  recorder.record(status);
  io.emit('status', status);
}, 1000);

createTerminus(server, {
  signal: 'SIGINT',
  healthChecks: {
    '/_health': onHealthCheck,
  },
  onSignal,
  onShutdown: async () => {
    log.info('Shutting down');
  },
});
