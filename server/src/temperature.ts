import fs from 'fs';

type DeviceTypes = "inside_temp" | "outside_temp";

export type Temperature = {
  [device in  DeviceTypes]: number;
}

export const getTemperature = (): Promise<Temperature> => {
  const temperatures = ["10-0008039a5582", "10-0008039e9723"]
    .map(device => readTemperature(device));
  
  return Promise.all(temperatures)
    .then(results => ({
      "inside_temp": results[0],
      "outside_temp": results[1]
    }));
};

const readTemperature = (device: string): Promise<number> => {
  return new Promise(resolve => {
    return fs.readFile(`/sys/bus/w1/devices/${device}/w1_slave`, 'utf-8', (err, data: string) => {
      if (err) {
        throw err;
      }

      const lines = data.split('\n');
      if (lines.length < 2) {
        throw new Error('Cannot read temperature sensor');
      }

      const isInvalidReading = !lines[0].endsWith('YES');
      if (isInvalidReading) {
        throw new Error(`CRC does not match ${lines[0]}`);
      }

      const match = lines[1].match(/t=(.*)/i);
      if (match === null) {
        throw new Error('No temperature reading found');
      }

      const temp = Math.round(100 * Number(match[1]) / 1000.0) / 100;
      resolve(temp);
    });
  });
}
