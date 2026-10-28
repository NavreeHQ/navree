# syntax=docker/dockerfile:1

# ---- build: compile and package with Maven, then extract the jar into
# the exploded layout the JDK's AOT cache needs.
FROM maven:3.10.0-eclipse-temurin-25 AS build
WORKDIR /src
# Builds share one Docker VM, so cap Maven's heap.
ENV MAVEN_OPTS="-Xmx1g"
COPY pom.xml ./
COPY src ./src
RUN --mount=type=cache,target=/root/.m2 \
    mvn -B -ntp -DskipTests package \
 && java -Djarmode=tools -jar target/app.jar extract --layers --destination /extracted

# ---- runtime
FROM eclipse-temurin:25-jre
WORKDIR /app
COPY --from=build /extracted/dependencies/ ./
COPY --from=build /extracted/spring-boot-loader/ ./
COPY --from=build /extracted/snapshot-dependencies/ ./
COPY --from=build /extracted/application/ ./

# AOT cache training run (JDK 25, JEP 483/514/515), as in Spring Boot's
# "Efficient deployments" docs: start the context, exit after refresh.
# There is no database at build time, so Hibernate is told the dialect
# instead of reading JDBC metadata; HikariCP connects lazily.
RUN DB_HOST=localhost DB_PORT=5432 DB_NAME=bench DB_USER=bench DB_PASSWORD=bench \
    JWT_SECRET=aot-training-only-0123456789abcdef0123456789abcdef \
    java -XX:AOTCacheOutput=app.aot -XX:MaxRAMPercentage=75 \
      -Dspring.context.exit=onRefresh \
      -Dspring.jpa.properties.hibernate.boot.allow_jdbc_metadata_access=false \
      -Dspring.jpa.database-platform=org.hibernate.dialect.PostgreSQLDialect \
      -jar app.jar

EXPOSE 8080
# A benchmark harness image: built and run only on a benchmarking machine, never deployed.
# nosemgrep: dockerfile.security.missing-user-entrypoint.missing-user-entrypoint
ENTRYPOINT ["java", "-XX:AOTCache=app.aot", "-XX:MaxRAMPercentage=75", "-jar", "app.jar"]
