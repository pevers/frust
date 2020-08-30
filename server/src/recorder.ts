import fs from 'fs';
import path from 'path';
import { getLogger } from './logging';

const log = getLogger(module);

// folder structure, e.g., 2020-04-19.log etc.
const LOGS = 'logs/';

export type StatusUpdate = {
  timestamp: string;
  status: "Idle" | "Cooling";
  target_temp: number;
  inside_temp: number;
  outside_temp: number;
  p: number;
  i: number;
  d: number;
}

export default class Recorder {
  statusUpdates: StatusUpdate[];

  constructor() {
    this.statusUpdates = [];
    this.setupRecorder();
  }

  public setupRecorder() {
    if (!fs.existsSync(LOGS)) {
      fs.mkdirSync(LOGS);
    }

    setInterval(() => {
      // Cleanup old logs every 24 hours
      const oneWeekAgo = new Date();
      oneWeekAgo.setTime(oneWeekAgo.getTime() - 7 * 24 * 3600_000);
      this.removeFileIfExists(path.join(LOGS, `${oneWeekAgo}.log`));
    }, 24 * 3600_000)
  }

  private removeFileIfExists(path: string) {
    try {
      fs.unlinkSync(path);
    } catch(e) {
      log.warn('Could not remove file. Maybe it did not exist');
    }
  }

  public record(statusUpdate: StatusUpdate) {
    const dateString = this.getDateString(new Date());
    const log = path.join(LOGS, `${dateString}.log`);
    const writeStream = fs.createWriteStream(log, {
      flags: 'a'
    });
    writeStream.write(this.toCsvLine(statusUpdate));
    writeStream.close();
  }

  private getDateString(date: Date) {
    return new Date(date.getTime() - (date.getTimezoneOffset() * 60000 ))
                    .toISOString()
                    .split("T")[0];
  }

  private toCsvLine(statusUpdate: StatusUpdate) {
    return `\n${statusUpdate.timestamp},${statusUpdate.status},${statusUpdate.inside_temp},${statusUpdate.outside_temp},${statusUpdate.target_temp},${statusUpdate.p},${statusUpdate.i},${statusUpdate.d}`;
  }
}

export async function getRecordedCsv(day: string) {
  return new Promise<string | null>((resolve, reject) => {
    fs.readFile(path.join(LOGS, `${day}.log`), { encoding: 'utf-8' }, (err, data) => {
      if (err) {
        resolve(null);
      }
      resolve(data);
    });
  });
}