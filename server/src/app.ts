import * as bodyParser from 'body-parser';
import express from 'express';
import { getRecordedCsv } from './recorder';
import { updateSettings } from './settings';

// Create Express server
const app = express();

app.use(bodyParser.json());
app.use(bodyParser.urlencoded({ extended: true }));

// Express configuration
app.set('port', process.env.PORT || 3000);

app.use(express.static('public'));

app.post('/temperature', (req: express.Request, res: express.Response) => {
  const required = ['target_temp', 'p', 'i', 'd'];
  required.forEach(required => {
    if (req.body[required] === undefined) {
      throw new Error(`Required var ${required} not set`);
    }
  });
  
  const target_temp = Number(req.body.target_temp);
  const [p,i,d] = [req.body.p, req.body.i, req.body.d];
  const settings = {
    target_temp,
    p,
    i,
    d
  };
  updateSettings(settings);
  console.log(`Updated settings to ${JSON.stringify(settings)}`);

  return res.send('OK');
});

app.get('/chart/:day', async (req: express.Request, res: express.Response) => {
  // Request CSV data for a certain day
  const day = req.params['day'];
  if (day === undefined) {
    throw new Error('No day specified');
  }

  const csv = await getRecordedCsv(day);
  if (!csv) {
    return res.json([]);
  }
  
  const data = csv.split('\n').map(line => {
    const tokens = line.split(',');
    return {
      timestamp: tokens[0],
      status: tokens[1],
      inside_temp: Number(tokens[2]),
      outside_temp: Number(tokens[3]),
      target_temp: Number(tokens[4])
    }
  })
  return res.json(data);
});

export default app;
