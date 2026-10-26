package bench.security;

import bench.user.User;
import bench.user.UserRepository;
import java.util.UUID;
import org.springframework.core.convert.converter.Converter;
import org.springframework.security.authentication.AbstractAuthenticationToken;
import org.springframework.security.oauth2.jwt.Jwt;
import org.springframework.security.oauth2.server.resource.InvalidBearerTokenException;
import org.springframework.stereotype.Component;

/** Turns a verified JWT into an authentication for the user named by {@code sub}; unknown users are rejected (401). */
@Component
public class UserJwtAuthenticationConverter implements Converter<Jwt, AbstractAuthenticationToken> {

    private final UserRepository users;

    public UserJwtAuthenticationConverter(UserRepository users) {
        this.users = users;
    }

    @Override
    public AbstractAuthenticationToken convert(Jwt jwt) {
        User user = users.findById(subject(jwt))
                .orElseThrow(() -> new InvalidBearerTokenException("Unknown user"));
        return new UserAuthenticationToken(jwt, user);
    }

    private static UUID subject(Jwt jwt) {
        String sub = jwt.getSubject();
        if (sub == null) {
            throw new InvalidBearerTokenException("Missing subject");
        }
        try {
            return UUID.fromString(sub);
        } catch (IllegalArgumentException e) {
            throw new InvalidBearerTokenException("Malformed subject");
        }
    }
}
