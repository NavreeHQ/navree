import { Controller, Get } from '@nestjs/common';

@Controller()
export class AppController {
  @Get('json')
  json(): { message: string } {
    return { message: 'Hello, World!' };
  }
}
